use std::{collections::HashMap, convert::Infallible, sync::Arc};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
};
use futures_util::StreamExt;
use serde_json::json;
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::{self, schema::ManagedAgentRow},
        runtime_refs::{self, schema::UpsertRuntimeRef},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    managed_agents::providers::{
        base::{validate_runtime, RuntimeProvision, RuntimeSessionInput, CLAUDE_AGENTS_RUNTIME},
        provision_runtime,
    },
    proxy::{auth::master_key::require_master_key, state::AppState},
    sdk::agents::{AgentSdkError, Lap, LapConfig},
};

use super::{
    session,
    types::{CreateSessionRequest, SessionResponse},
};

pub(super) async fn create_runtime_session(
    state: Arc<AppState>,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<SessionResponse, GatewayError> {
    let runtime = input.runtime.clone().unwrap_or_default();
    validate_runtime_request(&runtime)?;
    let agent = runtime_agent(pool, &input).await?;
    let credential = crate::http::agent_runtimes::load_credential(&state, &runtime).await?;
    let environment = input.environment.clone().unwrap_or_else(|| json!({}));
    let title = input.title.clone().unwrap_or_else(|| agent.name.clone());
    let row =
        insert_runtime_session(pool, &runtime, &agent, &input, &title, environment.clone()).await?;
    state.agent_runs.track_run(&agent.id, &row.id);
    let provision = provision_runtime(
        &state.http,
        &runtime,
        &agent,
        credential,
        RuntimeSessionInput {
            session_id: row.id.clone(),
            prompt: runtime_prompt(&input, &agent),
            environment,
        },
    )
    .await?;
    let row = attach_runtime_refs(pool, &row, &agent.id, &runtime, provision).await?;
    Ok(SessionResponse::from(row))
}

async fn runtime_agent(
    pool: &PgPool,
    input: &CreateSessionRequest,
) -> Result<ManagedAgentRow, GatewayError> {
    let agent_id = input
        .agent_id
        .clone()
        .or(input.agent.clone())
        .ok_or_else(|| GatewayError::InvalidJsonMessage("agent_id is required".to_owned()))?;
    registry::repository::get(pool, &agent_id)
        .await?
        .ok_or_else(|| GatewayError::UnknownAgent(agent_id))
}

async fn insert_runtime_session(
    pool: &PgPool,
    runtime: &str,
    agent: &ManagedAgentRow,
    input: &CreateSessionRequest,
    title: &str,
    environment: serde_json::Value,
) -> Result<SessionRow, GatewayError> {
    sessions::repository::create_runtime(
        pool,
        sessions::repository::CreateRuntimeSession {
            runtime,
            agent_id: &agent.id,
            title,
            timezone: input.timezone.as_deref().or(input.tz.as_deref()),
            runtime_agent_ref_id: None,
            environment,
            provider_session_id: None,
            provider_run_id: None,
        },
    )
    .await
}

fn runtime_prompt(input: &CreateSessionRequest, agent: &ManagedAgentRow) -> String {
    input
        .prompt
        .clone()
        .or_else(|| agent.prompt.clone())
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or_else(|| format!("Start a session for {}.", agent.name))
}

async fn attach_runtime_refs(
    pool: &PgPool,
    row: &SessionRow,
    agent_id: &str,
    runtime: &str,
    provision: RuntimeProvision,
) -> Result<SessionRow, GatewayError> {
    let provider_session_id = provision.provider_session_id.clone();
    let provider_run_id = provision.provider_run_id.clone();
    let runtime_ref = runtime_refs::repository::upsert(
        pool,
        agent_id,
        runtime,
        UpsertRuntimeRef {
            runtime_agent_id: provision.runtime_agent_id,
            provider_session_id: provider_session_id.clone(),
            provider_run_id: provider_run_id.clone(),
            provider_url: provision.provider_url,
            metadata: provision.metadata,
        },
    )
    .await?;
    sessions::repository::set_runtime_refs(
        pool,
        &row.id,
        &runtime_ref.id,
        provider_session_id.as_deref(),
        provider_run_id.as_deref(),
        "running",
    )
    .await
}

pub async fn runtime_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, GatewayError> {
    require_events_master_key(
        &headers,
        &query,
        state.config.general_settings.master_key.as_deref(),
    )?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let row = session(pool, &session_id).await?;
    if row.runtime.as_deref() != Some(CLAUDE_AGENTS_RUNTIME) {
        return Err(GatewayError::InvalidConfig(
            "runtime event streaming is only available for Claude Managed Agents sessions"
                .to_owned(),
        ));
    }
    let provider_session_id = row.provider_session_id.clone().ok_or_else(|| {
        GatewayError::InvalidConfig(
            "Claude Agents session is missing provider_session_id".to_owned(),
        )
    })?;
    let client = claude_agents_sdk_client(&state).await?;
    let provider_stream = client
        .beta()
        .sessions()
        .events()
        .stream(&provider_session_id)
        .await
        .map_err(agent_sdk_error)?;
    let body_stream = provider_stream
        .map(|event| Ok::<Bytes, Infallible>(Bytes::from(provider_event_line(event))));

    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(body_stream))
        .map_err(|error| GatewayError::SandboxError(error.to_string()))
}

pub(super) async fn claude_agents_sdk_client(state: &AppState) -> Result<Lap, GatewayError> {
    let credential =
        crate::http::agent_runtimes::load_credential(state, CLAUDE_AGENTS_RUNTIME).await?;
    Ok(Lap::new(LapConfig {
        anthropic_api_key: Some(credential.api_key),
        anthropic_base_url: credential.api_base,
    }))
}

pub(super) fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::SandboxError(format!(
            "managed agent provider request failed with status {status}: {body}"
        )),
        other => GatewayError::SandboxError(other.to_string()),
    }
}

fn validate_runtime_request(runtime: &str) -> Result<(), GatewayError> {
    if validate_runtime(runtime) {
        return Ok(());
    }
    Err(GatewayError::InvalidJsonMessage(format!(
        "unsupported runtime: {runtime}"
    )))
}

fn require_events_master_key(
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    configured: Option<&str>,
) -> Result<(), GatewayError> {
    if query.get("key").map(String::as_str) == configured {
        return Ok(());
    }
    require_master_key(headers, configured)
}

fn provider_event_line(event: Result<crate::sdk::agents::AgentEvent, AgentSdkError>) -> String {
    match event {
        Ok(event) => match serde_json::to_string(&event) {
            Ok(payload) => format!("data: {payload}\n\n"),
            Err(error) => error_line(error.to_string()),
        },
        Err(error) => error_line(error.to_string()),
    }
}

fn error_line(message: String) -> String {
    format!(
        "data: {}\n\n",
        json!({ "type": "session.error", "error": { "message": message } })
    )
}
