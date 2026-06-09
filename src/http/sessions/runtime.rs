use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::{self, schema::ManagedAgentRow},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    http::agent_runtimes::RuntimeCredential,
    proxy::state::AppState,
    sdk::providers,
};

use super::{
    runtime_provision::provision_runtime_session,
    runtime_sdk::{
        agent_sdk_error, provider_run_id, register_runtime_session, runtime_sdk_client,
        send_events_params,
    },
    storage::persist_message,
    types::{CreateSessionRequest, SessionResponse},
};

pub(super) struct CreatedRuntimeSession {
    pub(super) runtime: String,
    pub(super) agent: ManagedAgentRow,
    pub(super) credential: RuntimeCredential,
    pub(super) environment: Value,
    pub(super) initial_user_prompt: Option<String>,
    pub(super) prompt: String,
    pub(super) row: SessionRow,
}

pub(super) async fn create_runtime_session(
    state: Arc<AppState>,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<SessionResponse, GatewayError> {
    let created = create_runtime_session_row(&state, pool, input).await?;
    if let Some(prompt) = created.initial_user_prompt.as_deref() {
        persist_message(pool, &created.row.id, "user", prompt, None).await?;
    }
    let row = match provision_runtime_session(&state, pool, &created).await {
        Ok(row) => row,
        Err(error) => {
            let _ = sessions::repository::delete(pool, &created.row.id).await;
            return Err(error);
        }
    };
    if row.provider_run_id.is_none() {
        if let Some(prompt) = created.initial_user_prompt.as_deref() {
            execute_runtime_prompt(state.clone(), pool, row.clone(), prompt.to_owned()).await?;
        }
    }
    state.agent_runs.track_run(&created.agent.id, &row.id);
    Ok(SessionResponse::from(row))
}

pub(crate) async fn create_runtime_session_for_agent(
    state: Arc<AppState>,
    pool: &PgPool,
    agent_id: String,
    runtime: String,
    title: String,
    prompt: String,
    environment: Value,
) -> Result<String, GatewayError> {
    let runtime = registry::repository::get(pool, &agent_id)
        .await?
        .and_then(|agent| runtime_from_agent_config(&agent))
        .unwrap_or(runtime);
    let response = create_runtime_session(
        state,
        pool,
        CreateSessionRequest {
            title: Some(title),
            harness: None,
            agent: Some(agent_id.clone()),
            agent_id: Some(agent_id),
            runtime: Some(runtime),
            prompt: Some(prompt),
            environment: Some(environment),
            timezone: None,
            tz: None,
        },
    )
    .await?;
    Ok(response.id().to_owned())
}

fn runtime_from_agent_config(agent: &ManagedAgentRow) -> Option<String> {
    agent
        .config
        .get("runtime")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn create_runtime_session_row(
    state: &AppState,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<CreatedRuntimeSession, GatewayError> {
    let runtime = validated_runtime(&input)?;
    let mut agent = load_agent(pool, &input).await?;
    agent.system =
        crate::db::managed_agents::skills::compose::compose_agent_system_prompt(pool, &agent)
            .await?;
    let credential = crate::http::agent_runtimes::load_credential(state, &runtime).await?;
    let stored_environment = input.environment.clone().unwrap_or_else(|| json!({}));
    let title = input.title.clone().unwrap_or_else(|| agent.name.clone());
    let initial_user_prompt = input
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_owned);
    let row = sessions::repository::create_runtime(
        pool,
        sessions::repository::CreateRuntimeSession {
            runtime: &runtime,
            agent_id: &agent.id,
            title: &title,
            timezone: input.timezone.as_deref().or(input.tz.as_deref()),
            runtime_agent_ref_id: None,
            environment: stored_environment.clone(),
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
        environment: stored_environment,
        initial_user_prompt,
        prompt,
        row,
    })
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
    if let Some(run_id) = provider_run_id(runtime, &sent.raw) {
        sessions::repository::set_provider_run(pool, &row.id, &run_id, "running").await?;
    }
    Ok(())
}

fn validated_runtime(input: &CreateSessionRequest) -> Result<String, GatewayError> {
    let runtime = input.runtime.clone().unwrap_or_default();
    if providers::runtime_registry().validate_id(&runtime) {
        Ok(runtime)
    } else {
        Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported runtime: {runtime}"
        )))
    }
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
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or_else(|| format!("Start a session for {}.", agent.name))
}
