use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::{db::managed_agents::slack, errors::GatewayError, proxy::state::AppState};

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
    if let Some(event_id) = slack_event_id(payload) {
        if !slack::repository::record_event(&pool, &agent.id, event_id).await? {
            return Ok(());
        }
    }
    let row = slack::repository::ensure_thread_session(
        &pool,
        &agent.id,
        &agent.harness,
        &agent.timezone,
        &message.channel,
        &message.thread_ts,
    )
    .await?;
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
    Some(SlackIncomingMessage {
        channel: event.get("channel").and_then(Value::as_str)?.to_owned(),
        thread_ts: event
            .get("thread_ts")
            .or_else(|| event.get("ts"))
            .and_then(Value::as_str)?
            .to_owned(),
        prompt: clean_prompt(
            event
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
    })
}

fn is_supported_event(event: &Value) -> bool {
    match event.get("type").and_then(Value::as_str) {
        Some("app_mention") => true,
        Some("message") => matches!(
            event.get("channel_type").and_then(Value::as_str),
            Some("im" | "mpim")
        ),
        _ => false,
    }
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

fn slack_event_id(payload: &Value) -> Option<&str> {
    payload.get("event_id").and_then(Value::as_str)
}
