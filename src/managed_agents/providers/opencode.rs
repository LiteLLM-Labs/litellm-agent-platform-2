use serde_json::json;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    sdk::agents::{AgentSdkError, CreateSessionParams, Lap, LapConfig, OPENCODE},
};

use super::base::{RuntimeCredential, RuntimeProvision, RuntimeSessionInput, RuntimeTool};

pub const TOOLS: &[RuntimeTool] = &[];

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    _input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let client = Lap::with_http_client(
        LapConfig {
            opencode_api_key: Some(credential.api_key.clone()),
            opencode_base_url: Some(credential.api_base),
            opencode_password: Some(credential.api_key),
            ..LapConfig::default()
        },
        http.clone(),
    );
    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams::opencode(format!(
            "{} session",
            agent.name
        )))
        .await
        .map_err(agent_sdk_error)?;
    Ok(RuntimeProvision {
        runtime_agent_id: provider_session.id.clone(),
        provider_session_id: Some(provider_session.id),
        provider_run_id: None,
        provider_url: None,
        metadata: json!({
            "runtime": OPENCODE,
            "session": provider_session.raw,
        }),
    })
}

fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::InvalidConfig(format!(
            "opencode provider request failed with status {status}: {body}"
        )),
        other => GatewayError::InvalidConfig(other.to_string()),
    }
}
