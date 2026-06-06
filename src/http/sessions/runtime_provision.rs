use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::schema::ManagedAgentRow,
        runtime_refs::{self, schema::UpsertRuntimeRef},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    proxy::state::AppState,
    sdk::{
        agents::{
            AgentModel, AgentModelConfig, AgentRuntime, AgentWorkspace, CreateAgentParams,
            CreateEnvironmentParams, CreateSessionParams, Lap, LapConfig,
        },
        providers,
    },
};

use super::{runtime::CreatedRuntimeSession, runtime_sdk::agent_sdk_error};

struct RuntimeProvision {
    runtime_agent_id: String,
    provider_session_id: Option<String>,
    provider_run_id: Option<String>,
    provider_url: Option<String>,
    metadata: Value,
}

pub(super) async fn provision_runtime_session(
    state: &AppState,
    pool: &PgPool,
    created: &CreatedRuntimeSession,
) -> Result<SessionRow, GatewayError> {
    let sdk_rt = super::runtime_sdk::sdk_runtime(&created.runtime)?;
    let client = runtime_client(state, sdk_rt, created);
    let provider_agent = create_provider_agent(&client, sdk_rt, created).await?;
    let provider_env = create_provider_environment(&client, sdk_rt, created).await?;
    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: provider_agent.id.clone(),
            environment_id: provider_env.id.clone(),
            title: format!("{} session", created.agent.name),
            lap_agent_runtime: Some(sdk_rt),
            metadata: Some(session_metadata(
                &created.agent,
                &created.row.id,
                &created.prompt,
            )),
            resources: None,
        })
        .await
        .map_err(agent_sdk_error)?;
    let provision = runtime_provision(
        &created.runtime,
        &provider_agent.id,
        Some(provider_session.id.clone()),
        &provider_agent.raw,
        serde_json::json!({
            "runtime": created.runtime,
            "agent": provider_agent.raw,
            "environment": provider_env.raw,
            "session": provider_session.raw,
        }),
    );
    persist_runtime_refs(pool, created, provision).await
}

fn runtime_client(state: &AppState, runtime: AgentRuntime, created: &CreatedRuntimeSession) -> Lap {
    let mut config = LapConfig::default();
    match runtime {
        AgentRuntime::ClaudeManagedAgents => {
            config.anthropic_api_key = Some(created.credential.api_key.clone());
            config.anthropic_base_url = created.credential.api_base.clone();
        }
        AgentRuntime::Cursor => {
            config.cursor_api_key = Some(created.credential.api_key.clone());
            config.cursor_base_url = created.credential.api_base.clone();
        }
        AgentRuntime::OpenCode => {
            config.opencode_base_url = Some(created.credential.api_base.clone());
            config.opencode_api_key = Some(created.credential.api_key.clone());
            config.opencode_password = Some(created.credential.api_key.clone());
        }
    }
    Lap::with_http_client(config, state.http.clone())
}

async fn create_provider_agent(
    client: &Lap,
    runtime: AgentRuntime,
    created: &CreatedRuntimeSession,
) -> Result<crate::sdk::agents::ManagedAgent, GatewayError> {
    client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: runtime,
            lap_provider_options: None,
            name: created.agent.name.clone(),
            model: AgentModel::Config(AgentModelConfig {
                id: agent_model(&created.agent, &created.environment),
                speed: None,
            }),
            system: provider_system(runtime, created),
            description: created.agent.description.clone(),
            tools: vec![serde_json::json!({ "type": "agent_toolset_20260401" })],
            mcp_servers: mcp_servers(&created.agent),
            workspace: workspace_from_env(&created.environment)?,
            env_vars: None,
            metadata: Some(agent_metadata(&created.agent)),
        })
        .await
        .map_err(agent_sdk_error)
}

async fn create_provider_environment(
    client: &Lap,
    runtime: AgentRuntime,
    created: &CreatedRuntimeSession,
) -> Result<crate::sdk::agents::Environment, GatewayError> {
    client
        .beta()
        .environments()
        .create(CreateEnvironmentParams {
            lap_agent_runtime: runtime,
            name: format!("{} environment", created.agent.name),
            config: serde_json::json!({
                "type": "cloud",
                "networking": { "type": "unrestricted" }
            }),
            description: None,
            scope: None,
        })
        .await
        .map_err(agent_sdk_error)
}

fn runtime_provision(
    runtime: &str,
    agent_id: &str,
    provider_session_id: Option<String>,
    raw: &Value,
    metadata: Value,
) -> RuntimeProvision {
    let (provider_run_id, provider_url) = providers::runtime_registry()
        .entry_for_id(runtime)
        .map(|entry| {
            (
                entry.adapter.provider_run_id_from_agent_raw(raw),
                entry.adapter.provider_url_from_agent_raw(raw),
            )
        })
        .unwrap_or((None, None));
    RuntimeProvision {
        runtime_agent_id: agent_id.to_owned(),
        provider_session_id,
        provider_run_id,
        provider_url,
        metadata,
    }
}

async fn persist_runtime_refs(
    pool: &PgPool,
    created: &CreatedRuntimeSession,
    provision: RuntimeProvision,
) -> Result<SessionRow, GatewayError> {
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

fn provider_system(runtime: AgentRuntime, created: &CreatedRuntimeSession) -> String {
    if runtime != AgentRuntime::Cursor {
        return created.agent.system.clone();
    }
    let mut parts = Vec::new();
    if !created.agent.system.trim().is_empty() {
        parts.push(created.agent.system.trim().to_owned());
    }
    if let Some(context) = repository_context(&created.environment) {
        parts.push(context);
    }
    if !created.prompt.trim().is_empty() {
        parts.push(created.prompt.trim().to_owned());
    }
    parts.join("\n\n")
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
    let Some(repository) = repository_url(environment) else {
        return Ok(None);
    };
    if repository.trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "repository cannot be empty".to_owned(),
        ));
    }
    Ok(Some(AgentWorkspace {
        repository: repository.to_owned(),
        ref_name: ref_name(environment).map(str::to_owned),
        auto_create_pr: auto_create_pr(environment),
    }))
}

fn repository_context(environment: &Value) -> Option<String> {
    let repository = repository_url(environment)?;
    Some(format!(
        "Repository: {repository}\nBase branch: {}",
        ref_name(environment).unwrap_or("main")
    ))
}

fn repository_url(environment: &Value) -> Option<&str> {
    environment
        .get("repository")
        .and_then(Value::as_str)
        .or_else(|| source_field(environment, "repository"))
}

fn ref_name(environment: &Value) -> Option<&str> {
    environment
        .get("ref")
        .and_then(Value::as_str)
        .or_else(|| source_field(environment, "ref"))
}

fn source_field<'a>(environment: &'a Value, field: &str) -> Option<&'a str> {
    environment
        .get("source")
        .and_then(|source| source.get(field))
        .and_then(Value::as_str)
}

fn auto_create_pr(environment: &Value) -> bool {
    environment
        .get("auto_create_pr")
        .or_else(|| environment.get("autoCreatePr"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
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
