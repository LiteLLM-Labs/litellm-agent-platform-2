use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        runtime_refs::{self, schema::UpsertRuntimeRef},
        sessions::{self, schema::SessionRow},
    },
    errors::GatewayError,
    proxy::state::AppState,
    sdk::{
        agents::{
            AgentModel, AgentModelConfig, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
            CreateSessionParams, Lap, LapConfig,
        },
        providers,
    },
};

use super::{
    runtime::CreatedRuntimeSession,
    runtime_inputs::{
        agent_metadata, agent_model, integration_mcp_toolsets, mcp_servers,
        opencode_session_resources, provider_system, session_metadata, workspace_from_env,
    },
    runtime_sdk::agent_sdk_error,
};

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
    let provider_agent = create_provider_agent(state, &client, sdk_rt, created).await?;
    let provider_env = create_provider_environment(&client, sdk_rt, created).await?;
    let vault_ids = platform_mcp_vault_ids(state, sdk_rt, created).await?;
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
            vault_ids,
            resources: opencode_session_resources(state, sdk_rt, created)?,
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
    state: &AppState,
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
            tools: {
                let mut tools = vec![serde_json::json!({ "type": "agent_toolset_20260401" })];
                tools.extend(crate::http::platform_mcps::platform_mcp_toolsets(
                    &created.agent.config,
                    &created.agent.vault_keys,
                ));
                tools.extend(integration_mcp_toolsets(&created.agent.config));
                tools
            },
            mcp_servers: mcp_servers(state, &created.agent)?,
            workspace: workspace_from_env(&created.environment)?,
            env_vars: None,
            metadata: Some(agent_metadata(&created.agent)),
        })
        .await
        .map_err(agent_sdk_error)
}

async fn platform_mcp_vault_ids(
    state: &AppState,
    runtime: AgentRuntime,
    created: &CreatedRuntimeSession,
) -> Result<Option<Vec<String>>, GatewayError> {
    if runtime != AgentRuntime::ClaudeManagedAgents {
        return Ok(None);
    }
    if crate::http::platform_mcps::selected_platform_mcp_ids(&created.agent.config).is_empty()
        && crate::http::platform_mcps::vault_key_names(&created.agent.vault_keys).is_empty()
    {
        return Ok(None);
    }
    let token = state
        .config
        .general_settings
        .master_key
        .as_deref()
        .ok_or_else(|| {
            GatewayError::InvalidConfig(
                "master_key is required for platform MCP vault auth".to_owned(),
            )
        })?;
    let url = crate::http::platform_mcps::platform_mcp_url(state, &created.agent.id)?;
    let vault_id =
        create_platform_mcp_vault(state, &created.credential.api_key, &url, token).await?;
    Ok(Some(vec![vault_id]))
}

async fn create_platform_mcp_vault(
    state: &AppState,
    api_key: &str,
    mcp_server_url: &str,
    token: &str,
) -> Result<String, GatewayError> {
    let base = "https://api.anthropic.com/v1";
    let vault: Value = state
        .http
        .post(format!("{base}/vaults?beta=true"))
        .header("x-api-key", api_key)
        .header("anthropic-version", crate::sdk::agents::ANTHROPIC_VERSION)
        .header("anthropic-beta", crate::sdk::agents::MANAGED_AGENTS_BETA)
        .json(&serde_json::json!({ "display_name": "LiteLLM platform MCP" }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .error_for_status()
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    let vault_id = vault.get("id").and_then(Value::as_str).ok_or_else(|| {
        GatewayError::SandboxError("Anthropic vault response missing id".to_owned())
    })?;
    let credential = state
        .http
        .post(format!("{base}/vaults/{vault_id}/credentials?beta=true"))
        .header("x-api-key", api_key)
        .header("anthropic-version", crate::sdk::agents::ANTHROPIC_VERSION)
        .header("anthropic-beta", crate::sdk::agents::MANAGED_AGENTS_BETA)
        .json(&serde_json::json!({
            "auth": {
                "type": "static_bearer",
                "mcp_server_url": mcp_server_url,
                "token": token
            }
        }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?;
    if !credential.status().is_success() {
        let status = credential.status();
        let body = credential.text().await.unwrap_or_default();
        return Err(GatewayError::SandboxError(format!(
            "Anthropic vault credential create failed with status {status}: {body}"
        )));
    }
    Ok(vault_id.to_owned())
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
