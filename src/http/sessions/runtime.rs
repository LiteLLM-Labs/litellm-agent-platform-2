use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::{self, schema::ManagedAgentRow},
        runtime_refs::{self, schema::UpsertRuntimeRef},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    http::agent_runtimes::{load_credential, RuntimeCredential},
    proxy::state::AppState,
    sdk::{
        agents::{
            AgentModel, AgentModelConfig, AgentWorkspace, CreateAgentParams,
            CreateEnvironmentParams, CreateSessionParams,
        },
        providers,
    },
};

use super::{
    runtime_sdk::{
        agent_sdk_error, provider_run_id, register_runtime_session,
        runtime_sdk_client, sdk_runtime, send_events_params,
    },
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

struct RuntimeProvision {
    runtime_agent_id: String,
    provider_session_id: Option<String>,
    provider_run_id: Option<String>,
    provider_url: Option<String>,
    metadata: Value,
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

pub(crate) async fn create_runtime_session_for_agent(
    state: Arc<AppState>,
    pool: &PgPool,
    agent_id: String,
    title: String,
    prompt: String,
    environment: Value,
) -> Result<String, GatewayError> {
    let response = create_runtime_session(
        state,
        pool,
        CreateSessionRequest {
            title: Some(title),
            harness: None,
            agent: Some(agent_id.clone()),
            agent_id: Some(agent_id),
            runtime: Some(crate::sdk::agents::CLAUDE_MANAGED_AGENTS.to_owned()),
            prompt: Some(prompt),
            environment: Some(environment),
            timezone: None,
            tz: None,
        },
    )
    .await?;
    Ok(response.id().to_owned())
}

async fn create_runtime_session_row(
    state: &AppState,
    pool: &PgPool,
    input: CreateSessionRequest,
) -> Result<CreatedRuntimeSession, GatewayError> {
    let runtime = validated_runtime(&input)?;
    let mut agent = load_agent(pool, &input).await?;
    // Compose the agent's attached skills into its system prompt so the runtime
    // provider (e.g. claude_managed_agents) receives skill content downstream.
    // Without this the provider agent is created with the bare base system and
    // skills are silently dropped.
    agent.system =
        crate::db::managed_agents::skills::compose::compose_agent_system_prompt(pool, &agent)
            .await?;
    let credential = load_credential(state, &runtime).await?;
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
    let sdk_rt = sdk_runtime(&created.runtime)?;
    // Use state's shared HTTP client for connection reuse.
    let client = {
        use crate::sdk::agents::{Lap, LapConfig};
        let mut config = LapConfig::default();
        match sdk_rt {
            crate::sdk::agents::AgentRuntime::ClaudeManagedAgents => {
                config.anthropic_api_key = Some(created.credential.api_key.clone());
                config.anthropic_base_url = created.credential.api_base.clone();
            }
            crate::sdk::agents::AgentRuntime::Cursor => {
                config.cursor_api_key = Some(created.credential.api_key.clone());
                config.cursor_base_url = created.credential.api_base.clone();
            }
            crate::sdk::agents::AgentRuntime::OpenCode => {
                config.opencode_base_url = Some(created.credential.api_base.clone());
                config.opencode_api_key = Some(created.credential.api_key.clone());
                config.opencode_password = Some(created.credential.api_key.clone());
            }
        }
        Lap::with_http_client(config, state.http.clone())
    };

    let provider_agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: sdk_rt,
            lap_provider_options: None,
            name: created.agent.name.clone(),
            model: AgentModel::Config(AgentModelConfig {
                id: agent_model(&created.agent, &created.environment),
                speed: None,
            }),
            system: created.agent.system.clone(),
            description: created.agent.description.clone(),
            tools: vec![serde_json::json!({ "type": "agent_toolset_20260401" })],
            mcp_servers: mcp_servers(&created.agent),
            workspace: workspace_from_env(&created.environment)?,
            env_vars: None,
            metadata: Some(agent_metadata(&created.agent)),
        })
        .await
        .map_err(agent_sdk_error)?;

    let provider_env = client
        .beta()
        .environments()
        .create(CreateEnvironmentParams {
            lap_agent_runtime: sdk_rt,
            name: format!("{} environment", created.agent.name),
            config: serde_json::json!({
                "type": "cloud",
                "networking": { "type": "unrestricted" }
            }),
            description: None,
            scope: None,
        })
        .await
        .map_err(agent_sdk_error)?;

    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: provider_agent.id.clone(),
            environment_id: provider_env.id.clone(),
            title: format!("{} session", created.agent.name),
            lap_agent_runtime: Some(sdk_rt),
            metadata: Some(session_metadata(&created.agent, &created.row.id, &created.prompt)),
            resources: None,
        })
        .await
        .map_err(agent_sdk_error)?;

    // Delegate provider-specific extraction to the adapter.
    let (run_id, url) = providers::runtime_registry()
        .entry_for_id(&created.runtime)
        .map(|e| {
            (
                e.adapter.provider_run_id_from_agent_raw(&provider_agent.raw),
                e.adapter.provider_url_from_agent_raw(&provider_agent.raw),
            )
        })
        .unwrap_or((None, None));

    let provision = RuntimeProvision {
        runtime_agent_id: provider_agent.id.clone(),
        provider_session_id: Some(provider_session.id.clone()),
        provider_run_id: run_id,
        provider_url: url,
        metadata: serde_json::json!({
            "runtime": created.runtime,
            "agent": provider_agent.raw,
            "environment": provider_env.raw,
            "session": provider_session.raw,
        }),
    };

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
        .or_else(|| agent.prompt.clone())
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or_else(|| format!("Start a session for {}.", agent.name))
}

fn agent_model(agent: &ManagedAgentRow, environment: &Value) -> String {
    environment
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| agent.config.get("model").and_then(Value::as_str))
        .unwrap_or(&agent.model)
        .to_owned()
}

fn mcp_servers(agent: &ManagedAgentRow) -> Vec<Value> {
    let Some(value) = agent
        .config
        .get("mcp_servers")
        .or_else(|| agent.config.get("mcpServers"))
    else {
        return Vec::new();
    };
    if let Some(servers) = value.as_array() {
        return servers.clone();
    }
    value
        .as_object()
        .map(|servers| {
            servers
                .iter()
                .filter_map(|(name, server)| {
                    let mut server = server.as_object()?.clone();
                    server
                        .entry("name".to_owned())
                        .or_insert_with(|| Value::String(name.clone()));
                    Some(Value::Object(server))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn workspace_from_env(environment: &Value) -> Result<Option<AgentWorkspace>, GatewayError> {
    let repository = environment
        .get("repository")
        .and_then(Value::as_str)
        .or_else(|| {
            environment
                .get("source")
                .and_then(|s| s.get("repository"))
                .and_then(Value::as_str)
        });
    let Some(repository) = repository else {
        return Ok(None);
    };
    if repository.trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "repository cannot be empty".to_owned(),
        ));
    }
    let ref_name = environment
        .get("ref")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let auto_create_pr = environment
        .get("auto_create_pr")
        .or_else(|| environment.get("autoCreatePr"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Some(AgentWorkspace {
        repository: repository.to_owned(),
        ref_name,
        auto_create_pr,
    }))
}

fn agent_metadata(agent: &ManagedAgentRow) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("source".to_owned(), "litellm-agent-platform".to_owned()),
    ])
}

fn session_metadata(
    agent: &ManagedAgentRow,
    session_id: &str,
    prompt: &str,
) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("local_session_id".to_owned(), session_id.to_owned()),
        ("initial_prompt".to_owned(), prompt.to_owned()),
    ])
}
