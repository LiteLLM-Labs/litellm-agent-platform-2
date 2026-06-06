use serde_json::{json, Value};

use crate::{db::managed_agents::registry::schema::ManagedAgentRow, errors::GatewayError};

use super::base::{
    runtime_agent_id, selected_tool_ids, toolset_payload, RuntimeCredential, RuntimeProvision,
    RuntimeSessionInput, RuntimeTool, CLAUDE_AGENTS_RUNTIME,
};

const MANAGED_AGENTS_BETA: &str = "managed-agents-2026-04-01";
const AGENT_TOOLSET: &str = "agent_toolset_20260401";
pub const TOOLS: &[RuntimeTool] = &[
    RuntimeTool {
        id: "bash",
        name: "Shell",
        description: "Run shell commands in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "read",
        name: "Read files",
        description: "Read files from the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "write",
        name: "Write files",
        description: "Create or overwrite files in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "edit",
        name: "Edit files",
        description: "Patch existing files in the agent environment.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "glob",
        name: "Find files",
        description: "Find files by glob pattern.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "grep",
        name: "Search files",
        description: "Search file contents by regular expression.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "web_fetch",
        name: "Fetch URL",
        description: "Fetch content from a URL.",
        enabled_by_default: true,
    },
    RuntimeTool {
        id: "web_search",
        name: "Web search",
        description: "Search the web for information.",
        enabled_by_default: true,
    },
];

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let base = credential.api_base.trim_end_matches('/');
    let provider_agent = create_agent(http, base, &credential.api_key, agent).await?;
    let provider_environment = create_environment(http, base, &credential.api_key, agent).await?;
    let provider_session = create_session(
        http,
        base,
        &credential.api_key,
        agent,
        &provider_agent,
        &provider_environment,
        &input,
    )
    .await?;
    let agent_id =
        id_from(&provider_agent).unwrap_or_else(|| runtime_agent_id(agent, CLAUDE_AGENTS_RUNTIME));
    Ok(RuntimeProvision {
        runtime_agent_id: agent_id,
        provider_session_id: id_from(&provider_session),
        provider_run_id: None,
        provider_url: None,
        metadata: json!({
            "runtime": CLAUDE_AGENTS_RUNTIME,
            "agent": provider_agent,
            "environment": provider_environment,
            "session": provider_session,
        }),
    })
}

async fn create_agent(
    http: &reqwest::Client,
    base: &str,
    api_key: &str,
    agent: &ManagedAgentRow,
) -> Result<Value, GatewayError> {
    let body = json!({
        "name": agent.name,
        "model": { "id": agent.model },
        "system": agent.system,
        "tools": agent_tools(agent),
        "metadata": {
            "local_agent_id": agent.id,
            "source": "litellm-agent-platform"
        }
    });
    post_managed(http, base, api_key, "/v1/agents", body).await
}

fn agent_tools(agent: &ManagedAgentRow) -> Value {
    let tools = configured_tools(agent);
    toolset_payload(
        AGENT_TOOLSET,
        &selected_tool_ids(&tools, TOOLS, &[AGENT_TOOLSET]),
    )
}

fn configured_tools(agent: &ManagedAgentRow) -> Value {
    if let Some(tools) = agent.config.get("tools") {
        return tools.clone();
    }
    if should_use_default_tools(agent) {
        return Value::Null;
    }
    agent.tools.clone()
}

fn should_use_default_tools(agent: &ManagedAgentRow) -> bool {
    agent.tools.as_array().is_some_and(Vec::is_empty) && agent.config.get("tools").is_none()
}

async fn create_environment(
    http: &reqwest::Client,
    base: &str,
    api_key: &str,
    agent: &ManagedAgentRow,
) -> Result<Value, GatewayError> {
    let body = json!({
        "name": format!("{} environment", agent.name),
        "config": {
            "type": "cloud",
            "networking": { "type": "unrestricted" }
        },
        "metadata": {
            "local_agent_id": agent.id,
            "source": "litellm-agent-platform"
        }
    });
    post_managed(http, base, api_key, "/v1/environments", body).await
}

async fn create_session(
    http: &reqwest::Client,
    base: &str,
    api_key: &str,
    agent: &ManagedAgentRow,
    provider_agent: &Value,
    provider_environment: &Value,
    input: &RuntimeSessionInput,
) -> Result<Value, GatewayError> {
    let provider_agent_id = id_from(provider_agent).ok_or_else(|| {
        GatewayError::InvalidConfig("Claude Agents create agent response missing id".to_owned())
    })?;
    let provider_environment_id = id_from(provider_environment).ok_or_else(|| {
        GatewayError::InvalidConfig(
            "Claude Agents create environment response missing id".to_owned(),
        )
    })?;
    let body = json!({
        "agent": provider_agent_id,
        "environment_id": provider_environment_id,
        "title": format!("{} session", agent.name),
        "metadata": {
            "local_agent_id": agent.id,
            "local_session_id": input.session_id,
            "initial_prompt": input.prompt,
        }
    });
    post_managed(http, base, api_key, "/v1/sessions", body).await
}

async fn post_managed(
    http: &reqwest::Client,
    base: &str,
    api_key: &str,
    path: &str,
    body: Value,
) -> Result<Value, GatewayError> {
    let response = http
        .post(format!("{base}{path}"))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .header("x-api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(GatewayError::Upstream)?;
    let status = response.status();
    let payload = response
        .json::<Value>()
        .await
        .map_err(GatewayError::Upstream)?;
    if !status.is_success() {
        return Err(GatewayError::InvalidConfig(format!(
            "Claude Agents request failed at {path}: {payload}"
        )));
    }
    Ok(payload)
}

fn id_from(value: &Value) -> Option<String> {
    value.get("id").and_then(Value::as_str).map(str::to_owned)
}
