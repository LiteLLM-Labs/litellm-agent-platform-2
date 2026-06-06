use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::{
    db::managed_agents::slack, errors::GatewayError,
    http::sessions::create_runtime_session_for_agent, proxy::state::AppState,
};

use super::{
    config::{load_agent, load_secret, signing_secret_key, slack_config},
    replies::spawn_slack_prompt,
    signature,
    types::{SlackAgentConfig, SlackIncomingMessage},
};

pub async fn events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let pool = state
        .db
        .as_ref()
        .ok_or(GatewayError::MissingDatabase)?
        .clone();
    let agent = load_agent(&pool, &agent_id).await?;
    let config = slack_config(&agent)?;
    let secret = load_secret(&state, &signing_secret_key(&agent.id, &config)).await?;
    signature::verify(&headers, &body, &secret)?;
    let payload: Value = serde_json::from_slice(&body)?;

    if payload.get("type").and_then(Value::as_str) == Some("url_verification") {
        return Ok((StatusCode::OK, challenge(&payload)).into_response());
    }
    if payload.get("type").and_then(Value::as_str) == Some("event_callback") {
        handle_event_callback(state, pool, agent, config, &payload).await?;
    }
    Ok(StatusCode::OK.into_response())
}

async fn handle_event_callback(
    state: Arc<AppState>,
    pool: sqlx::PgPool,
    agent: crate::db::managed_agents::registry::schema::ManagedAgentRow,
    config: SlackAgentConfig,
    payload: &Value,
) -> Result<(), GatewayError> {
    let Some(message) = incoming_message(payload) else {
        return Ok(());
    };
    let event_key = slack_event_key(payload, &message);
    if !slack::repository::record_event(&pool, &agent.id, &event_key).await? {
        return Ok(());
    }
    let row = match message.requires_existing_thread {
        true => {
            match slack::repository::get(&pool, &agent.id, &message.channel, &message.thread_ts)
                .await?
            {
                Some(row) => row,
                None => return Ok(()),
            }
        }
        false => {
            let session_id = create_runtime_session_for_agent(
                state.clone(),
                &pool,
                agent.id.clone(),
                format!("Slack {} {}", message.channel, message.thread_ts),
                message.prompt.clone(),
                serde_json::json!({
                    "source": "slack",
                    "channel_id": message.channel,
                    "thread_ts": message.thread_ts,
                }),
            )
            .await?;
            slack::repository::upsert(
                &pool,
                &agent.id,
                &message.channel,
                &message.thread_ts,
                &session_id,
            )
            .await?
        }
    };
    spawn_slack_prompt(state, pool, agent, config, message, row.session_id);
    Ok(())
}

fn incoming_message(payload: &Value) -> Option<SlackIncomingMessage> {
    let event = payload.get("event")?;
    if event.get("bot_id").is_some() || event.get("subtype").is_some() {
        return None;
    }
    if !is_supported_event(event) {
        return None;
    }
    let channel = event.get("channel").and_then(Value::as_str)?.to_owned();
    Some(SlackIncomingMessage {
        thread_ts: session_thread_ts(event, &channel)?,
        reply_thread_ts: reply_thread_ts(event)?,
        channel,
        prompt: clean_prompt(
            event
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        requires_existing_thread: is_thread_reply(event),
    })
}

fn is_supported_event(event: &Value) -> bool {
    match event.get("type").and_then(Value::as_str) {
        Some("app_mention") => true,
        Some("message") => is_direct_message(event) || is_thread_reply(event),
        _ => false,
    }
}

fn session_thread_ts(event: &Value, _channel: &str) -> Option<String> {
    reply_thread_ts(event)
}

fn reply_thread_ts(event: &Value) -> Option<String> {
    event
        .get("thread_ts")
        .or_else(|| event.get("ts"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_direct_message(event: &Value) -> bool {
    matches!(
        event.get("channel_type").and_then(Value::as_str),
        Some("im" | "mpim")
    )
}

fn is_thread_reply(event: &Value) -> bool {
    if event.get("type").and_then(Value::as_str) != Some("message") {
        return false;
    }
    let Some(thread_ts) = event.get("thread_ts").and_then(Value::as_str) else {
        return false;
    };
    let ts = event.get("ts").and_then(Value::as_str);
    ts != Some(thread_ts)
}

fn clean_prompt(text: &str) -> String {
    let prompt = text
        .split_whitespace()
        .filter(|part| !part.starts_with("<@"))
        .collect::<Vec<_>>()
        .join(" ");
    match prompt.trim() {
        "" => "Proceed with your task.".to_owned(),
        _ => prompt,
    }
}

fn challenge(payload: &Value) -> String {
    payload
        .get("challenge")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn slack_event_key(payload: &Value, message: &SlackIncomingMessage) -> String {
    payload
        .get("event_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| fallback_event_key(payload, message))
}

fn fallback_event_key(payload: &Value, message: &SlackIncomingMessage) -> String {
    let event = payload.get("event").unwrap_or(&Value::Null);
    let ts = event
        .get("event_ts")
        .or_else(|| event.get("ts"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let user = event
        .get("user")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = event
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!(
        "fallback:{}:{}:{}:{}:{}",
        message.channel, message.thread_ts, ts, user, text
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::incoming_message;

    #[test]
    fn direct_messages_use_message_thread() {
        let first = incoming_message(&json!({
            "event": {
                "type": "message",
                "channel_type": "im",
                "channel": "D123",
                "ts": "1.000001",
                "text": "hello"
            }
        }))
        .unwrap();
        let second = incoming_message(&json!({
            "event": {
                "type": "message",
                "channel_type": "im",
                "channel": "D123",
                "ts": "1.000002",
                "text": "again"
            }
        }))
        .unwrap();
        assert_eq!(first.thread_ts, "1.000001");
        assert_eq!(second.thread_ts, "1.000002");
        assert_eq!(first.reply_thread_ts, "1.000001");
        assert_eq!(second.reply_thread_ts, "1.000002");
    }

    #[test]
    fn direct_message_thread_replies_reuse_existing_thread() {
        let message = incoming_message(&json!({
            "event": {
                "type": "message",
                "channel_type": "im",
                "channel": "D123",
                "thread_ts": "1.000001",
                "ts": "1.000002",
                "text": "follow up"
            }
        }))
        .unwrap();
        assert_eq!(message.thread_ts, "1.000001");
        assert_eq!(message.reply_thread_ts, "1.000001");
        assert!(message.requires_existing_thread);
    }

    #[test]
    fn threaded_mentions_can_create_sessions() {
        let message = incoming_message(&json!({
            "event": {
                "type": "app_mention",
                "channel_type": "channel",
                "channel": "C123",
                "thread_ts": "1.000001",
                "ts": "1.000002",
                "text": "<@B123> hello"
            }
        }))
        .unwrap();
        assert_eq!(message.thread_ts, "1.000001");
        assert!(!message.requires_existing_thread);
    }
}
