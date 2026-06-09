use serde_json::Value;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    proxy::state::AppState,
    sdk::agents::{AgentRuntime, AgentWorkspace},
};

use super::runtime::CreatedRuntimeSession;
use super::runtime_mcp_validation::validate_runtime_mcp_servers;

pub(super) fn provider_system(runtime: AgentRuntime, created: &CreatedRuntimeSession) -> String {
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

pub(super) fn agent_model(agent: &ManagedAgentRow, environment: &Value) -> String {
    environment
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| agent.config.get("model").and_then(Value::as_str))
        .unwrap_or(&agent.model)
        .to_owned()
}

pub(super) fn mcp_servers(
    state: &AppState,
    agent: &ManagedAgentRow,
    session_id: Option<&str>,
) -> Result<Vec<Value>, GatewayError> {
    let Some(value) = agent
        .config
        .get("mcp_servers")
        .or_else(|| agent.config.get("mcpServers"))
    else {
        return crate::http::platform_mcps::platform_mcp_servers(
            state,
            &agent.id,
            &agent.config,
            session_id,
        );
    };
    let mut servers = if let Some(servers) = value.as_array() {
        servers.clone()
    } else {
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
    };
    servers.extend(crate::http::platform_mcps::platform_mcp_servers(
        state,
        &agent.id,
        &agent.config,
        session_id,
    )?);
    rewrite_registered_mcp_servers(state, &mut servers)?;
    validate_runtime_mcp_servers(&agent.id, &servers)?;
    Ok(servers)
}

/// Registered MCP servers attached to an agent store the *raw* upstream URL,
/// which may carry `${VAR}` placeholders resolved per-user at call time (e.g.
/// Composio's `${COMPOSIO_USER_ID}` / `${COMPOSIO_MCP_SERVER_ID}`). The
/// managed-agents runtime (Anthropic) calls the MCP URL directly and rejects an
/// unresolved `${...}` URL as an invalid URI. So any templated entry is
/// rewritten to route through this gateway's own MCP proxy
/// (`{proxy_base}/{name}/mcp`), which resolves the caller's vault variables,
/// injects the server's static headers, and forwards upstream — the same path
/// tool discovery already uses. The proxy requires a gateway key, supplied as
/// the entry's `authorization_token` (mirrors the platform-MCP auth pattern).
/// `name` is the server id; the dynamic proxy resolves it by id, name, or alias.
///
/// v0: on-behalf-of identity is the default owner. Per-user identity over this
/// path is tracked separately (signed-token auth) — see issue.
fn rewrite_registered_mcp_servers(
    state: &AppState,
    servers: &mut [Value],
) -> Result<(), GatewayError> {
    for server in servers.iter_mut() {
        let Some(obj) = server.as_object_mut() else {
            continue;
        };
        let needs_proxy = obj
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("${"));
        if !needs_proxy {
            continue;
        }
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                GatewayError::InvalidConfig(
                    "mcp_servers entry with ${variables} requires a name (server id)".to_owned(),
                )
            })?;
        let base = state.resolved_mcp_proxy_base_url().ok_or_else(|| {
            GatewayError::InvalidConfig(
                "mcp_servers.proxy_base_url is required to proxy MCP servers with variables"
                    .to_owned(),
            )
        })?;
        obj.insert(
            "url".to_owned(),
            Value::String(format!("{}/{}/mcp", base.trim_end_matches('/'), name)),
        );
        if let Some(key) = state.config.general_settings.master_key.as_deref() {
            obj.entry("authorization_token".to_owned())
                .or_insert_with(|| Value::String(key.to_owned()));
        }
    }
    Ok(())
}

pub(super) fn workspace_from_env(
    environment: &Value,
) -> Result<Option<AgentWorkspace>, GatewayError> {
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

pub(super) fn agent_metadata(agent: &ManagedAgentRow) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("source".to_owned(), "litellm-agent-platform".to_owned()),
    ])
}

pub(super) fn session_metadata(
    agent: &ManagedAgentRow,
    session_id: &str,
    prompt: &str,
) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("local_session_id".to_owned(), session_id.to_owned()),
        ("initial_prompt".to_owned(), metadata_value(prompt)),
    ])
}

pub(super) fn opencode_session_resources(
    state: &AppState,
    runtime: AgentRuntime,
    created: &CreatedRuntimeSession,
) -> Result<Option<Value>, GatewayError> {
    if runtime != AgentRuntime::OpenCode {
        return Ok(None);
    }
    Ok(Some(serde_json::json!({
        "agent": {
            "id": &created.agent.id,
            "name": &created.agent.name,
            "description": &created.agent.description,
        },
        "system": &created.agent.system,
        "model": agent_model(&created.agent, &created.environment),
        "tools": &created.agent.tools,
        "mcp_servers": mcp_servers(state, &created.agent, Some(&created.row.id))?,
        "environment": &created.environment,
    })))
}

fn metadata_value(value: &str) -> String {
    const MAX_CHARS: usize = 512;
    value.chars().take(MAX_CHARS).collect()
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

pub fn integration_mcp_toolsets(config: &Value) -> Vec<Value> {
    let server_names: std::collections::HashSet<&str> = config
        .get("mcp_servers")
        .and_then(Value::as_array)
        .map(|servers| {
            servers
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    config
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter(|t| {
                    t.get("type").and_then(Value::as_str) == Some("mcp_toolset")
                        && t.get("mcp_server_name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| server_names.contains(name))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::db::managed_agents::registry::schema::ManagedAgentRow;

    use super::session_metadata;

    #[test]
    fn session_metadata_truncates_long_prompt_values() {
        let agent = ManagedAgentRow {
            id: "agent_1".to_owned(),
            name: "Agent".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            system: String::new(),
            tools: json!([]),
            cadence: None,
            interval_seconds: None,
            session_id: String::new(),
            loop_id: None,
            created_at: 0,
            prompt: None,
            cron: None,
            timezone: "UTC".to_owned(),
            vault_keys: json!([]),
            setup_commands: json!([]),
            max_runtime_minutes: 30,
            on_failure: "notify".to_owned(),
            config: json!({}),
            owner_id: Some("owner".to_owned()),
            status: "active".to_owned(),
            description: None,
            harness: "claude-code".to_owned(),
            skill_ids: json!([]),
            rule_ids: json!([]),
        };
        let metadata = session_metadata(&agent, "ses_1", &"x".repeat(600));
        assert_eq!(metadata["initial_prompt"].chars().count(), 512);
    }
}
