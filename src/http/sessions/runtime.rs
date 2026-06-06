use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::{self, schema::ManagedAgentRow},
        runtime_refs::{self, schema::UpsertRuntimeRef},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    managed_agents::providers::{
        base::{normalize_runtime, RuntimeCredential, RuntimeSessionInput},
        provision_runtime,
    },
    proxy::{auth::master_key::require_master_key, state::AppState},
};

use super::{
    runtime_sdk::{
        agent_sdk_error, provider_event_line, provider_run_id, register_runtime_session,
        runtime_sdk_client, send_events_params,
    },
    storage::session,
    types::{CreateSessionRequest, SessionResponse},
};

struct CreatedRuntimeSession {
    runtime: String,
    agent: ManagedAgentRow,
    credential: RuntimeCredential,
    environment: Value,
    prompt: String,
    row: SessionRow,
}

pub(super) async fn create_runtime_session(
    state: Arc<AppState>,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<SessionResponse, GatewayError> {
    let created = create_runtime_session_row(&state, pool, input).await?;
    let row = match provision_runtime_session(&state, pool, &created).await {
        Ok(row) => row,
        Err(error) => {
            let _ = sessions::repository::delete(pool, &created.row.id).await;
            return Err(error);
        }
    };
    state.agent_runs.track_run(&created.agent.id, &row.id);
    Ok(SessionResponse::from(row))
}

async fn create_runtime_session_row(
    state: &AppState,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<CreatedRuntimeSession, GatewayError> {
    let runtime = validated_runtime(&input)?;
    let agent = load_agent(pool, &input).await?;
    let credential = crate::http::agent_runtimes::load_credential(state, &runtime).await?;
    let environment = input.environment.clone().unwrap_or_else(|| json!({}));
    let title = input.title.clone().unwrap_or_else(|| agent.name.clone());
    let row = sessions::repository::create_runtime(
        pool,
        sessions::repository::CreateRuntimeSession {
            runtime: &runtime,
            agent_id: &agent.id,
            title: &title,
            timezone: input.timezone.as_deref().or(input.tz.as_deref()),
            runtime_agent_ref_id: None,
            environment: environment.clone(),
            provider_session_id: None,
            provider_run_id: None,
        },
    )
    .await?;
    let prompt = runtime_prompt(input.prompt, &agent);
    Ok(CreatedRuntimeSession {
        runtime,
        agent,
        credential,
        environment,
        prompt,
        row,
    })
}

async fn provision_runtime_session(
    state: &AppState,
    pool: &PgPool,
    created: &CreatedRuntimeSession,
) -> Result<SessionRow, GatewayError> {
    let provision = provision_runtime(
        &state.http,
        &created.runtime,
        &created.agent,
        created.credential.clone(),
        RuntimeSessionInput {
            session_id: created.row.id.clone(),
            prompt: created.prompt.clone(),
            environment: created.environment.clone(),
        },
    )
    .await?;
    let runtime_ref = runtime_refs::repository::upsert(
        pool,
        &created.agent.id,
        &created.runtime,
        UpsertRuntimeRef {
            runtime_agent_id: provision.runtime_agent_id,
            provider_session_id: provision.provider_session_id.clone(),
            provider_run_id: provision.provider_run_id.clone(),
            provider_url: provision.provider_url,
            metadata: provision.metadata,
        },
    )
    .await?;
    sessions::repository::set_runtime_refs(
        pool,
        &created.row.id,
        &runtime_ref.id,
        provision.provider_session_id.as_deref(),
        provision.provider_run_id.as_deref(),
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
    let runtime = row.runtime.as_deref().ok_or_else(|| {
        GatewayError::InvalidConfig("session is not a runtime session".to_owned())
    })?;
    let client = runtime_sdk_client(&state, runtime).await?;
    register_runtime_session(&client, &row)?;
    let provider_stream = client
        .beta()
        .sessions()
        .events()
        .stream(&row.id)
        .await
        .map_err(agent_sdk_error)?;
    let body_stream = provider_stream.map(provider_event_line);
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(body_stream))
        .map_err(|error| GatewayError::SandboxError(error.to_string()))
}

pub(super) async fn execute_runtime_prompt(
    state: Arc<AppState>,
    pool: &PgPool,
    row: SessionRow,
    prompt: String,
) -> Result<(), GatewayError> {
    let runtime = row.runtime.as_deref().ok_or_else(|| {
        GatewayError::InvalidConfig("runtime session is missing runtime".to_owned())
    })?;
    let client = runtime_sdk_client(&state, runtime).await?;
    register_runtime_session(&client, &row)?;
    state
        .agent_runs
        .update_status(&row.id, crate::agents::runs::AgentRunStatus::Running);
    let sent = client
        .beta()
        .sessions()
        .events()
        .send(&row.id, send_events_params(prompt))
        .await
        .map_err(agent_sdk_error)?;
    if let Some(provider_run_id) = provider_run_id(runtime, &sent.raw) {
        sessions::repository::set_provider_run(pool, &row.id, &provider_run_id, "running").await?;
    }
    Ok(())
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

fn validated_runtime(input: &CreateSessionRequest) -> Result<String, GatewayError> {
    let runtime = input.runtime.clone().unwrap_or_default();
    normalize_runtime(&runtime)
        .map(str::to_owned)
        .ok_or_else(|| GatewayError::InvalidJsonMessage(format!("unsupported runtime: {runtime}")))
}

async fn load_agent(
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
        .ok_or_else(|| GatewayError::UnknownAgent(agent_id.clone()))
}

fn runtime_prompt(prompt: Option<String>, agent: &ManagedAgentRow) -> String {
    prompt
        .or_else(|| agent.prompt.clone())
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or_else(|| format!("Start a session for {}.", agent.name))
}
