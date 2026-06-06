use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::json;
use sqlx::PgPool;

use crate::{
    agents::events,
    db::managed_agents::{
        messages, registry,
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    proxy::{auth::master_key::require_master_key, state::AppState},
};

mod prompt;
mod runtime;
mod types;

pub use runtime::runtime_events;

use types::{CreateSessionRequest, MessageResponse, PromptRequest, SessionResponse};

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
    let pool = db(&state, &headers)?.clone();
    if input.runtime.is_some() {
        return runtime::create_runtime_session(state, &pool, input)
            .await
            .map(Json);
    }
    let resolved = resolve_session_request(&pool, input).await?;
    let row = sessions::repository::create(
        &pool,
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
        .model_id()
        .unwrap_or_else(|| "claude-sonnet-4-6".to_owned());

    prompt::persist_message(&pool, &session_id, "user", &prompt, None).await?;
    state
        .agent_runs
        .track_run(row.agent_id.as_deref().unwrap_or(&row.harness), &session_id);

    tokio::spawn(async move {
        if let Err(error) = prompt::execute_prompt(state.clone(), pool, row, prompt, model).await {
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

pub(super) async fn session(pool: &PgPool, session_id: &str) -> Result<SessionRow, GatewayError> {
    sessions::repository::get(pool, session_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("session not found".to_owned()))
}

fn db<'a>(state: &'a AppState, headers: &HeaderMap) -> Result<&'a PgPool, GatewayError> {
    require_master_key(headers, state.config.general_settings.master_key.as_deref())?;
    state.db.as_ref().ok_or(GatewayError::MissingDatabase)
}

#[derive(Debug)]
struct ResolvedSession {
    title: String,
    harness: String,
    agent_id: Option<String>,
    timezone: Option<String>,
}
