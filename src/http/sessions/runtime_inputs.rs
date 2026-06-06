use serde_json::Value;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    sdk::agents::{AgentRuntime, AgentWorkspace},
};

use super::runtime::CreatedRuntimeSession;

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

pub(super) fn mcp_servers(agent: &ManagedAgentRow) -> Vec<Value> {
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
        ("initial_prompt".to_owned(), prompt.to_owned()),
    ])
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
