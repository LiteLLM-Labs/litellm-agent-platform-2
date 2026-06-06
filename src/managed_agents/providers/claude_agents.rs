use std::collections::HashMap;

use serde_json::{json, Value};

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    sdk::agents::{
        AgentModel, AgentModelConfig, AgentRuntime, AgentSdkError, CreateAgentParams,
        CreateSessionParams, Environment, Lap, LapConfig,
    },
};

use super::base::{
    RuntimeCredential, RuntimeProvision, RuntimeSessionInput, CLAUDE_AGENTS_RUNTIME,
};

const PLATFORM_SOURCE: &str = "litellm-agent-platform";

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let client = Lap::with_http_client(
        LapConfig {
            anthropic_api_key: Some(credential.api_key),
            anthropic_base_url: credential.api_base,
            ..LapConfig::default()
        },
        http.clone(),
    );

    let provider_agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            lap_provider_options: Some(json!({ "metadata": agent_metadata(agent) })),
            name: agent.name.clone(),
            model: AgentModel::Config(AgentModelConfig {
                id: agent.model.clone(),
                speed: None,
            }),
            system: agent.system.clone(),
            description: None,
            tools: vec![json!({ "type": "agent_toolset_20260401" })],
            mcp_servers: Vec::new(),
        })
        .await
        .map_err(agent_sdk_error)?;

    let environment_raw = client
        .post(
            AgentRuntime::ClaudeManagedAgents,
            "/v1/environments",
            &json!({
                "name": format!("{} environment", agent.name),
                "config": {
                    "type": "cloud",
                    "networking": { "type": "unrestricted" }
                },
                "metadata": agent_metadata(agent),
            }),
        )
        .await
        .map_err(agent_sdk_error)?;
    let provider_environment = Environment {
        id: id_from(&environment_raw)?,
        raw: environment_raw,
    };

    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: provider_agent.id.clone(),
            environment_id: provider_environment.id.clone(),
            title: format!("{} session", agent.name),
            lap_agent_runtime: Some(AgentRuntime::ClaudeManagedAgents),
            metadata: Some(session_metadata(agent, &input)),
            resources: None,
        })
        .await
        .map_err(agent_sdk_error)?;

    Ok(RuntimeProvision {
        runtime_agent_id: provider_agent.id.clone(),
        provider_session_id: Some(provider_session.id.clone()),
        provider_run_id: None,
        provider_url: None,
        metadata: json!({
            "runtime": CLAUDE_AGENTS_RUNTIME,
            "agent": provider_agent.raw,
            "environment": provider_environment.raw,
            "session": provider_session.raw,
        }),
    })
}

fn agent_metadata(agent: &ManagedAgentRow) -> HashMap<String, String> {
    HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("source".to_owned(), PLATFORM_SOURCE.to_owned()),
    ])
}

fn session_metadata(
    agent: &ManagedAgentRow,
    input: &RuntimeSessionInput,
) -> HashMap<String, String> {
    HashMap::from([
        ("local_agent_id".to_owned(), agent.id.clone()),
        ("local_session_id".to_owned(), input.session_id.clone()),
        ("initial_prompt".to_owned(), input.prompt.clone()),
    ])
}

fn id_from(value: &Value) -> Result<String, GatewayError> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            GatewayError::InvalidConfig("Claude Agents provider response missing id".to_owned())
        })
}

fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::InvalidConfig(format!(
            "Claude Agents provider request failed with status {status}: {body}"
        )),
        AgentSdkError::MissingId => {
            GatewayError::InvalidConfig("Claude Agents provider response missing id".to_owned())
        }
        AgentSdkError::MissingField(field) => {
            GatewayError::InvalidConfig(format!("Claude Agents provider response missing {field}"))
        }
        other => GatewayError::InvalidConfig(other.to_string()),
    }
}
