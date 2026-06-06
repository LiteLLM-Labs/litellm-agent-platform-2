mod signature;
mod web_api;

use std::{sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::warn;

use crate::{
    agents::events as agent_events,
    db::managed_agents::{
        inbox,
        registry::{
            self,
            schema::{ManagedAgentRow, UpdateManagedAgent},
        },
        sessions, slack,
    },
    errors::GatewayError,
    http::sessions::enqueue_prompt_text,
    proxy::{state::AppState, vault},
};

const DEFAULT_VAULT_USER: &str = "default";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct SlackAgentConfig {
    app_name: Option<String>,
    app_id: Option<String>,
    client_id: Option<String>,
    provider_id: Option<String>,
    status: Option<String>,
    client_secret_key: Option<String>,
    signing_secret_key: Option<String>,
    bot_token_key: Option<String>,
    slack_team_name: Option<String>,
    bot_user_id: Option<String>,
    oauth_error: Option<String>,
}

#[derive(Debug, Clone)]
struct SlackIncomingMessage {
    channel: String,
    thread_ts: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackInteractionPayload {
    actions: Option<Vec<SlackInteractionAction>>,
}

#[derive(Debug, Deserialize)]
struct SlackInteractionAction {
    action_id: String,
    value: Option<String>,
}

#[derive(Debug, Serialize)]
struct SlackInteractionResponse {
    text: String,
}

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
    let slack_config = slack_config(&agent)?;
    let signing_secret = load_secret(&state, &signing_secret_key(&agent.id, &slack_config)).await?;
    signature::verify(&headers, &body, &signing_secret)?;
    let payload: Value = serde_json::from_slice(&body)?;

    if payload.get("type").and_then(Value::as_str) == Some("url_verification") {
        let challenge = payload
            .get("challenge")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        return Ok((StatusCode::OK, challenge).into_response());
    }

    if payload.get("type").and_then(Value::as_str) == Some("event_callback") {
        if let Some(message) = incoming_message(&payload) {
            let session_id = ensure_thread_session(&pool, &agent, &message).await?;
            spawn_slack_prompt(state, pool, agent, slack_config, message, session_id);
        }
    }

    Ok(StatusCode::OK.into_response())
}

pub async fn interactivity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let agent = load_agent(pool, &agent_id).await?;
    let slack_config = slack_config(&agent)?;
    let signing_secret = load_secret(&state, &signing_secret_key(&agent.id, &slack_config)).await?;
    signature::verify(&headers, &body, &signing_secret)?;
    let payload_json = form_field(&body, "payload")
        .ok_or_else(|| GatewayError::InvalidJsonMessage("payload is required".to_owned()))?;
    let payload: SlackInteractionPayload = serde_json::from_str(&payload_json)?;
    let mut text = "No action taken.".to_owned();

    for action in payload.actions.unwrap_or_default() {
        if let Some((decision, item_id)) = approval_action(&action) {
            let live = match decision {
                "accept" => {
                    inbox::repository::decide_approval(pool, &item_id, "accept", None, None).await?
                }
                "reject" => {
                    inbox::repository::decide_approval(
                        pool,
                        &item_id,
                        "reject",
                        Some("Rejected from Slack".to_owned()),
                        None,
                    )
                    .await?
                }
                _ => false,
            };
            text = if live {
                format!("Approval {decision}ed.")
            } else {
                "Approval was already handled.".to_owned()
            };
        }
    }

    Ok(Json(SlackInteractionResponse { text }).into_response())
}

pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(provider_id): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Redirect, GatewayError> {
    let agent_id = query
        .state
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GatewayError::InvalidJsonMessage("missing oauth state".to_owned()))?;
    if provider_id_for(agent_id) != provider_id {
        return Err(GatewayError::Unauthorized);
    }
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let agent = load_agent(pool, agent_id).await?;
    let slack_config = slack_config(&agent)?;

    if let Some(error) = query.error {
        update_slack_config(
            pool,
            &agent,
            json!({ "status": "oauth_failed", "oauth_error": error }),
        )
        .await?;
        return Ok(Redirect::to("/agents/?slack=failed"));
    }

    let code = query
        .code
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GatewayError::InvalidJsonMessage("missing oauth code".to_owned()))?;
    let client_id = slack_config
        .client_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidConfig("slack client_id is not configured".to_owned())
        })?;
    let client_secret = load_secret(&state, &client_secret_key(&agent.id, &slack_config)).await?;
    let redirect_uri = format!("{}/host-oauth-callback/{provider_id}", origin(&headers));
    let oauth = web_api::oauth_access(
        &state.http,
        &state.config.slack.api_base_url,
        client_id,
        &client_secret,
        code,
        &redirect_uri,
    )
    .await?;

    if !oauth.ok {
        let error = oauth.error.unwrap_or_else(|| "oauth_failed".to_owned());
        update_slack_config(
            pool,
            &agent,
            json!({ "status": "oauth_failed", "oauth_error": error }),
        )
        .await?;
        return Ok(Redirect::to("/agents/?slack=failed"));
    }

    let access_token = oauth.access_token.ok_or_else(|| {
        GatewayError::InvalidConfig("slack oauth response omitted access_token".to_owned())
    })?;
    let bot_token_key = bot_token_key(&agent.id, &slack_config);
    vault::save(
        pool,
        &state.config,
        DEFAULT_VAULT_USER,
        &bot_token_key,
        &access_token,
    )
    .await?;
    update_slack_config(
        pool,
        &agent,
        json!({
            "status": "connected",
            "bot_token_key": bot_token_key,
            "slack_team_name": oauth.team.and_then(|team| team.name),
            "bot_user_id": oauth.bot_user_id,
            "oauth_error": Value::Null,
        }),
    )
    .await?;
    Ok(Redirect::to("/agents/?slack=connected"))
}

fn spawn_slack_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    agent: ManagedAgentRow,
    slack_config: SlackAgentConfig,
    message: SlackIncomingMessage,
    session_id: String,
) {
    tokio::spawn(async move {
        if let Err(error) =
            run_slack_prompt(state, pool, agent, slack_config, message, session_id).await
        {
            warn!("slack prompt failed: {error}");
        }
    });
}

async fn run_slack_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    agent: ManagedAgentRow,
    slack_config: SlackAgentConfig,
    message: SlackIncomingMessage,
    session_id: String,
) -> Result<(), GatewayError> {
    let bot_token = load_secret(&state, &bot_token_key(&agent.id, &slack_config)).await?;
    let event_stream = state.agent_runs.event_stream();
    let placeholder_ts = match web_api::post_message(
        &state.http,
        &state.config.slack.api_base_url,
        &bot_token,
        &message.channel,
        &message.thread_ts,
        "_Thinking..._",
    )
    .await
    {
        Ok(ts) => Some(ts),
        Err(error) => {
            warn!("slack placeholder failed: {error}");
            None
        }
    };

    let enqueue = enqueue_prompt_text(
        state.clone(),
        pool,
        &session_id,
        message.prompt.clone(),
        agent.model.clone(),
    )
    .await;
    if let Err(error) = enqueue {
        if let Some(ts) = placeholder_ts.as_deref() {
            let _ = web_api::update_message(
                &state.http,
                &state.config.slack.api_base_url,
                &bot_token,
                &message.channel,
                ts,
                &format!("Agent failed to start: {error}"),
            )
            .await;
        }
        return Err(error);
    }

    if let Some(ts) = placeholder_ts {
        stream_reply_to_slack(
            &state,
            event_stream.rx,
            &bot_token,
            &message.channel,
            &ts,
            &session_id,
        )
        .await?;
    }
    Ok(())
}

async fn stream_reply_to_slack(
    state: &AppState,
    mut rx: broadcast::Receiver<String>,
    bot_token: &str,
    channel: &str,
    ts: &str,
    session_id: &str,
) -> Result<(), GatewayError> {
    let mut text = String::new();
    let mut since_update = tokio::time::Instant::now();
    loop {
        match rx.recv().await {
            Ok(line) => {
                let Some((event_type, properties)) = event_payload(&line) else {
                    continue;
                };
                if properties.get("sessionID").and_then(Value::as_str) != Some(session_id) {
                    continue;
                }
                match event_type.as_str() {
                    agent_events::MESSAGE_PART_DELTA => {
                        if let Some(delta) = properties.get("delta").and_then(Value::as_str) {
                            text.push_str(delta);
                        }
                        if since_update.elapsed() >= Duration::from_secs(1) && !text.is_empty() {
                            web_api::update_message(
                                &state.http,
                                &state.config.slack.api_base_url,
                                bot_token,
                                channel,
                                ts,
                                &text,
                            )
                            .await?;
                            since_update = tokio::time::Instant::now();
                        }
                    }
                    agent_events::SESSION_ERROR => {
                        let message = properties
                            .get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("Agent run failed.");
                        web_api::update_message(
                            &state.http,
                            &state.config.slack.api_base_url,
                            bot_token,
                            channel,
                            ts,
                            message,
                        )
                        .await?;
                        return Ok(());
                    }
                    agent_events::SESSION_IDLE => {
                        web_api::update_message(
                            &state.http,
                            &state.config.slack.api_base_url,
                            bot_token,
                            channel,
                            ts,
                            text.trim().if_empty("Done."),
                        )
                        .await?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

fn event_payload(line: &str) -> Option<(String, Value)> {
    let data = line.strip_prefix("data: ")?;
    let payload: Value = serde_json::from_str(data.trim()).ok()?;
    let event_type = payload.get("type")?.as_str()?.to_owned();
    let properties = payload.get("properties")?.clone();
    Some((event_type, properties))
}

async fn ensure_thread_session(
    pool: &PgPool,
    agent: &ManagedAgentRow,
    message: &SlackIncomingMessage,
) -> Result<String, GatewayError> {
    if let Some(row) =
        slack::repository::get(pool, &agent.id, &message.channel, &message.thread_ts).await?
    {
        slack::repository::upsert(
            pool,
            &agent.id,
            &message.channel,
            &message.thread_ts,
            &row.session_id,
        )
        .await?;
        return Ok(row.session_id);
    }
    let title = format!("Slack {} {}", message.channel, message.thread_ts);
    let row = sessions::repository::create(
        pool,
        &agent.harness,
        Some(&agent.id),
        &title,
        Some(&agent.timezone),
    )
    .await?;
    slack::repository::upsert(
        pool,
        &agent.id,
        &message.channel,
        &message.thread_ts,
        &row.id,
    )
    .await?;
    Ok(row.id)
}

fn incoming_message(payload: &Value) -> Option<SlackIncomingMessage> {
    let event = payload.get("event")?;
    if event.get("bot_id").is_some() || event.get("subtype").is_some() {
        return None;
    }
    let event_type = event.get("type").and_then(Value::as_str)?;
    let supported = event_type == "app_mention"
        || (event_type == "message"
            && matches!(
                event.get("channel_type").and_then(Value::as_str),
                Some("im" | "mpim")
            ));
    if !supported {
        return None;
    }
    let channel = event.get("channel").and_then(Value::as_str)?.to_owned();
    let thread_ts = event
        .get("thread_ts")
        .or_else(|| event.get("ts"))
        .and_then(Value::as_str)?
        .to_owned();
    let prompt = clean_prompt(
        event
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    Some(SlackIncomingMessage {
        channel,
        thread_ts,
        prompt,
    })
}

fn clean_prompt(text: &str) -> String {
    let prompt = text
        .split_whitespace()
        .filter(|part| !part.starts_with("<@"))
        .collect::<Vec<_>>()
        .join(" ");
    if prompt.trim().is_empty() {
        "Proceed with your task.".to_owned()
    } else {
        prompt
    }
}

async fn load_agent(pool: &PgPool, agent_id: &str) -> Result<ManagedAgentRow, GatewayError> {
    registry::repository::get(pool, agent_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("agent not found".to_owned()))
}

fn slack_config(agent: &ManagedAgentRow) -> Result<SlackAgentConfig, GatewayError> {
    serde_json::from_value(
        agent
            .config
            .get("slack")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(GatewayError::InvalidJson)
}

async fn load_secret(state: &AppState, key: &str) -> Result<String, GatewayError> {
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    vault::load(pool, &state.config, DEFAULT_VAULT_USER, key)
        .await?
        .ok_or_else(|| GatewayError::InvalidConfig(format!("vault key is not configured: {key}")))
}

fn signing_secret_key(agent_id: &str, config: &SlackAgentConfig) -> String {
    config
        .signing_secret_key
        .clone()
        .unwrap_or_else(|| format!("SLACK_{agent_id}_SIGNING_SECRET"))
}

fn client_secret_key(agent_id: &str, config: &SlackAgentConfig) -> String {
    config
        .client_secret_key
        .clone()
        .unwrap_or_else(|| format!("SLACK_{agent_id}_CLIENT_SECRET"))
}

fn bot_token_key(agent_id: &str, config: &SlackAgentConfig) -> String {
    config
        .bot_token_key
        .clone()
        .unwrap_or_else(|| format!("SLACK_{agent_id}_BOT_TOKEN"))
}

async fn update_slack_config(
    pool: &PgPool,
    agent: &ManagedAgentRow,
    patch: Value,
) -> Result<(), GatewayError> {
    let config = patched_slack_config(&agent.config, patch);
    registry::repository::update(
        pool,
        &agent.id,
        UpdateManagedAgent {
            name: None,
            model: None,
            system: None,
            prompt: None,
            cron: None,
            timezone: None,
            vault_keys: None,
            setup_commands: None,
            max_runtime_minutes: None,
            on_failure: None,
            config: Some(config),
            owner_id: None,
            status: None,
            description: None,
            harness: None,
            skill_ids: None,
        },
    )
    .await?;
    Ok(())
}

fn patched_slack_config(config: &Value, patch: Value) -> Value {
    let mut root = config.as_object().cloned().unwrap_or_default();
    let mut slack = root
        .get("slack")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(patch) = patch.as_object() {
        for (key, value) in patch {
            slack.insert(key.clone(), value.clone());
        }
    }
    root.insert("slack".to_owned(), Value::Object(slack));
    Value::Object(root)
}

fn approval_action(action: &SlackInteractionAction) -> Option<(&'static str, String)> {
    let decision = match action.action_id.as_str() {
        "lap_approval_accept" | "approval_accept" => "accept",
        "lap_approval_reject" | "approval_reject" => "reject",
        _ => return None,
    };
    let value = action.value.as_deref()?;
    let item_id = serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| {
            value
                .get("item_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| value.to_owned());
    Some((decision, item_id))
}

fn form_field(body: &[u8], key: &str) -> Option<String> {
    let body = std::str::from_utf8(body).ok()?;
    for pair in body.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(raw_key)? == key {
            return percent_decode(raw_value);
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn provider_id_for(agent_id: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in agent_id.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    output
}

fn origin(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .or_else(|| headers.get("x-forwarded-protocol"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

trait EmptyText {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyText for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}
