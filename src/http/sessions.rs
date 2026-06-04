use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    agents::{
        config::AgentDefinition,
        events,
        harnesses::{build_harness_run, HarnessEvent, HarnessRunContext},
        runs::AgentRunStatus,
        sandboxes::{SandboxCommand, SandboxRunner},
    },
    db::managed_agents::{
        messages,
        registry::{self, schema::ManagedAgentRow},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    proxy::{auth::master_key::require_master_key, state::AppState},
};

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionResponse>>, GatewayError> {
    let pool = db(&state, &headers)?;
    let rows = sessions::repository::list(pool).await?;
    Ok(Json(rows.into_iter().map(SessionResponse::from).collect()))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>, GatewayError> {
    let pool = db(&state, &headers)?;
    let resolved = resolve_session_request(pool, input).await?;
    let row = sessions::repository::create(
        pool,
        &resolved.harness,
        resolved.agent_id.as_deref(),
        &resolved.title,
        resolved.timezone.as_deref(),
    )
    .await?;
    state.agent_runs.track_run(&resolved.harness, &row.id);
    Ok(Json(SessionResponse::from(row)))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<SessionResponse>, GatewayError> {
    let pool = db(&state, &headers)?;
    let row = session(pool, &session_id).await?;
    Ok(Json(SessionResponse::from(row)))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<bool>, GatewayError> {
    let pool = db(&state, &headers)?;
    Ok(Json(sessions::repository::delete(pool, &session_id).await?))
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<MessageResponse>>, GatewayError> {
    let pool = db(&state, &headers)?;
    let rows = messages::repository::list(pool, &session_id).await?;
    rows.into_iter()
        .map(MessageResponse::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

pub async fn prompt_async(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(input): Json<PromptRequest>,
) -> Result<StatusCode, GatewayError> {
    let pool = db(&state, &headers)?.clone();
    let row = session(&pool, &session_id).await?;
    let prompt = input.prompt_text()?;
    let model = input
        .model
        .as_ref()
        .map(|model| model.model_id.clone())
        .unwrap_or_else(|| "claude-sonnet-4-6".to_owned());

    persist_message(&pool, &session_id, "user", &prompt, None).await?;
    state
        .agent_runs
        .track_run(row.agent_id.as_deref().unwrap_or(&row.harness), &session_id);

    tokio::spawn(async move {
        if let Err(error) = execute_prompt(state.clone(), pool, row, prompt, model).await {
            let message = error.to_string();
            state.agent_runs.set_error(&session_id, message.clone());
            state.agent_runs.push_event(
                &session_id,
                events::SESSION_ERROR,
                json!({ "error": { "message": message } }),
            );
            state
                .agent_runs
                .push_event(&session_id, events::SESSION_IDLE, json!({}));
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(input): Json<PromptRequest>,
) -> Result<Json<Vec<MessageResponse>>, GatewayError> {
    prompt_async(
        State(state.clone()),
        headers.clone(),
        Path(session_id.clone()),
        Json(input),
    )
    .await?;
    let pool = db(&state, &headers)?;
    let rows = messages::repository::list(pool, &session_id).await?;
    rows.into_iter()
        .map(MessageResponse::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

pub async fn abort(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<StatusCode, GatewayError> {
    let _ = db(&state, &headers)?;
    state
        .agent_runs
        .set_error(&session_id, "aborted".to_owned());
    state.agent_runs.push_event(
        &session_id,
        events::SESSION_ERROR,
        json!({ "error": { "name": "MessageAbortedError", "message": "aborted" } }),
    );
    state
        .agent_runs
        .push_event(&session_id, events::SESSION_IDLE, json!({}));
    Ok(StatusCode::NO_CONTENT)
}

async fn execute_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    row: SessionRow,
    prompt: String,
    model: String,
) -> Result<(), GatewayError> {
    let agent = agent_definition(&pool, &state, &row, &model).await?;
    let mut harness_run = build_harness_run(&agent, &prompt)?;
    let context = HarnessRunContext::new(&row.id);
    push_events(&state, &row.id, harness_run.events.start(&context));

    let sandbox = SandboxRunner::from_settings(state.http.clone(), &state.config.general_settings)?;
    let session = sandbox.create(&row.id).await?;
    if let Some(sandbox_id) = session.sandbox_id.clone() {
        state.agent_runs.set_sandbox_id(&row.id, sandbox_id);
    }
    state
        .agent_runs
        .update_status(&row.id, AgentRunStatus::Running);

    let mut assistant_text = String::new();
    let run_result = async {
        let mut stream = sandbox
            .start(
                &session,
                SandboxCommand {
                    command: harness_run.command,
                },
            )
            .await?;
        while let Some(output) = stream.next().await {
            let output = output?;
            if output.delta.is_empty() {
                continue;
            }
            let events = harness_run.events.output(&context, output);
            for event in &events {
                if event.event == events::MESSAGE_PART_DELTA {
                    if let Some(delta) = event.data.get("delta").and_then(Value::as_str) {
                        assistant_text.push_str(delta);
                    }
                }
            }
            push_events(&state, &row.id, events);
        }
        Ok::<(), GatewayError>(())
    }
    .await;

    let _ = sandbox.terminate(&session).await;
    run_result?;

    if !assistant_text.is_empty() {
        persist_message_with_ids(
            &pool,
            &row.id,
            "assistant",
            &assistant_text,
            Some("stop"),
            Some(&context.message_id),
            Some(&context.part_id),
        )
        .await?;
    }
    state
        .agent_runs
        .update_status(&row.id, AgentRunStatus::Completed);
    push_events(&state, &row.id, harness_run.events.complete(&context));
    Ok(())
}

fn push_events(state: &AppState, session_id: &str, events: Vec<HarnessEvent>) {
    for event in events {
        state
            .agent_runs
            .push_event(session_id, event.event, event.data);
    }
}

async fn agent_definition(
    pool: &PgPool,
    state: &AppState,
    row: &SessionRow,
    model: &str,
) -> Result<AgentDefinition, GatewayError> {
    if let Some(agent_id) = row.agent_id.as_deref() {
        if let Some(agent) = registry::repository::get(pool, agent_id).await? {
            return Ok(managed_agent_definition(agent));
        }
        if let Some(agent) = state
            .config
            .agents
            .iter()
            .find(|agent| agent.id() == agent_id)
        {
            return Ok(agent.clone());
        }
    }

    Ok(AgentDefinition {
        id: Some(row.id.clone()),
        name: row.title.clone(),
        description: None,
        model: model.to_owned(),
        harness: Some(row.harness.clone()),
        system: String::new(),
        mcp_servers: Vec::new(),
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    })
}

fn managed_agent_definition(agent: ManagedAgentRow) -> AgentDefinition {
    AgentDefinition {
        id: Some(agent.id),
        name: agent.name,
        description: agent.description,
        model: agent.model,
        harness: Some(agent.harness),
        system: agent.system,
        mcp_servers: Vec::new(),
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    }
}

async fn resolve_session_request(
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<ResolvedSession, GatewayError> {
    let requested = input.agent.or(input.harness);
    if let Some(agent_id) = requested
        .as_deref()
        .filter(|value| value.starts_with("agent_"))
    {
        let agent = registry::repository::get(pool, agent_id)
            .await?
            .ok_or_else(|| GatewayError::UnknownAgent(agent_id.to_owned()))?;
        return Ok(ResolvedSession {
            title: input.title.unwrap_or(agent.name),
            harness: agent.harness,
            agent_id: Some(agent.id),
            timezone: input.timezone.or(input.tz),
        });
    }

    let harness = requested
        .filter(|value| value == "claude-code")
        .unwrap_or_else(|| "claude-code".to_owned());
    Ok(ResolvedSession {
        title: input.title.unwrap_or_else(|| "New session".to_owned()),
        harness,
        agent_id: None,
        timezone: input.timezone.or(input.tz),
    })
}

async fn persist_message(
    pool: &PgPool,
    session_id: &str,
    role: &str,
    text: &str,
    finish: Option<&str>,
) -> Result<(), GatewayError> {
    persist_message_with_ids(pool, session_id, role, text, finish, None, None).await
}

async fn persist_message_with_ids(
    pool: &PgPool,
    session_id: &str,
    role: &str,
    text: &str,
    finish: Option<&str>,
    message_id: Option<&str>,
    part_id: Option<&str>,
) -> Result<(), GatewayError> {
    let message_id = message_id
        .map(str::to_owned)
        .unwrap_or_else(|| crate::db::managed_agents::id("msg"));
    let part_id = part_id
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{message_id}_text"));
    let now = crate::db::managed_agents::now_ms();
    let mut info = json!({
        "id": message_id,
        "role": role,
        "sessionID": session_id,
        "time": { "created": now },
    });
    if let Some(finish) = finish {
        info["finish"] = finish.into();
        info["time"]["completed"] = now.into();
    }
    let parts = json!([{
        "id": part_id,
        "messageID": message_id,
        "sessionID": session_id,
        "type": "text",
        "text": text,
    }]);
    messages::repository::append(pool, session_id, &info.to_string(), &parts.to_string()).await?;
    Ok(())
}

async fn session(pool: &PgPool, session_id: &str) -> Result<SessionRow, GatewayError> {
    sessions::repository::get(pool, session_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("session not found".to_owned()))
}

fn db<'a>(state: &'a AppState, headers: &HeaderMap) -> Result<&'a PgPool, GatewayError> {
    require_master_key(headers, state.config.general_settings.master_key.as_deref())?;
    state.db.as_ref().ok_or(GatewayError::MissingDatabase)
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    title: Option<String>,
    harness: Option<String>,
    agent: Option<String>,
    timezone: Option<String>,
    tz: Option<String>,
}

#[derive(Debug)]
struct ResolvedSession {
    title: String,
    harness: String,
    agent_id: Option<String>,
    timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    model: Option<PromptModel>,
    parts: Option<Vec<PromptPart>>,
}

impl PromptRequest {
    fn prompt_text(&self) -> Result<String, GatewayError> {
        let text = self
            .parts
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|part| match part {
                PromptPart::Text { text } => Some(text.as_str()),
                PromptPart::Other => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            return Err(GatewayError::InvalidJsonMessage(
                "prompt text is required".to_owned(),
            ));
        }
        Ok(text)
    }
}

#[derive(Debug, Deserialize)]
struct PromptModel {
    #[serde(rename = "modelID")]
    model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PromptPart {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    id: String,
    title: String,
    agent: String,
    agent_id: Option<String>,
    harness: String,
    time: SessionTime,
}

impl From<SessionRow> for SessionResponse {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            agent: row.agent_id.clone().unwrap_or_else(|| row.harness.clone()),
            agent_id: row.agent_id,
            harness: row.harness,
            time: SessionTime {
                created: row.created_at,
                updated: row.updated_at,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct SessionTime {
    created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    info: Value,
    parts: Value,
}

impl TryFrom<messages::schema::SessionMessageRow> for MessageResponse {
    type Error = GatewayError;

    fn try_from(row: messages::schema::SessionMessageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            info: serde_json::from_str(&row.info_json)?,
            parts: serde_json::from_str(&row.parts_json)?,
        })
    }
}
