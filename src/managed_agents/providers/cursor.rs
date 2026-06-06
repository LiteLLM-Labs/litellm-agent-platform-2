use serde_json::{json, Value};

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    sdk::agents::{
        AgentModel, AgentRuntime, AgentSdkError, CreateAgentParams, CreateSessionParams, Lap,
        LapConfig,
    },
};

use super::base::{RuntimeCredential, RuntimeProvision, RuntimeSessionInput, CURSOR_RUNTIME};

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let client = Lap::with_http_client(
        LapConfig {
            cursor_api_key: Some(credential.api_key),
            cursor_base_url: credential.api_base,
            ..LapConfig::default()
        },
        http.clone(),
    );
    let provider_agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::Cursor,
            name: agent.name.clone(),
            model: AgentModel::from(cursor_model(agent, &input.environment)),
            system: cursor_prompt(agent, &input.prompt),
            description: agent.description.clone(),
            tools: Vec::new(),
            mcp_servers: mcp_servers(agent),
        })
        .await
        .map_err(agent_sdk_error)?;
    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: provider_agent.id.clone(),
            environment_id: input.session_id.clone(),
            title: format!("{} session", agent.name),
            lap_agent_runtime: Some(AgentRuntime::Cursor),
            metadata: None,
            resources: None,
        })
        .await
        .map_err(agent_sdk_error)?;

    Ok(RuntimeProvision {
        runtime_agent_id: provider_agent.id.clone(),
        provider_session_id: Some(provider_session.id),
        provider_run_id: nested_string(&provider_agent.raw, "run", "id"),
        provider_url: provider_url(&provider_agent.raw),
        metadata: json!({
            "runtime": CURSOR_RUNTIME,
            "agent": provider_agent.raw,
            "session": provider_session.raw,
        }),
    })
}

fn cursor_model(agent: &ManagedAgentRow, environment: &Value) -> String {
    environment
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| agent.config.get("model").and_then(Value::as_str))
        .unwrap_or(&agent.model)
        .to_owned()
}

fn cursor_prompt(agent: &ManagedAgentRow, prompt: &str) -> String {
    if agent.system.trim().is_empty() {
        prompt.to_owned()
    } else {
        format!("{}\n\n{}", agent.system.trim(), prompt)
    }
}

fn mcp_servers(agent: &ManagedAgentRow) -> Vec<Value> {
    agent
        .config
        .get("mcp_servers")
        .or_else(|| agent.config.get("mcpServers"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn provider_url(raw: &Value) -> Option<String> {
    raw.get("url")
        .and_then(Value::as_str)
        .or_else(|| raw.get("webUrl").and_then(Value::as_str))
        .or_else(|| {
            raw.get("agent")
                .and_then(|agent| agent.get("url"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            raw.get("agent")
                .and_then(|agent| agent.get("webUrl"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
}

fn nested_string(raw: &Value, parent: &str, field: &str) -> Option<String> {
    raw.get(parent)
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::InvalidConfig(format!(
            "cursor provider request failed with status {status}: {body}"
        )),
        other => GatewayError::InvalidConfig(other.to_string()),
    }
}
