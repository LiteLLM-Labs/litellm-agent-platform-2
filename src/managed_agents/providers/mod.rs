pub mod base;
pub mod claude_agents;
pub mod cursor;

use crate::{db::managed_agents::registry::schema::ManagedAgentRow, errors::GatewayError};

use self::base::{
    validate_runtime, RuntimeCredential, RuntimeProvision, RuntimeSessionInput,
    CLAUDE_AGENTS_RUNTIME, CURSOR_RUNTIME,
};

pub async fn provision_runtime(
    http: &reqwest::Client,
    runtime: &str,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    if !validate_runtime(runtime) {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported runtime: {runtime}"
        )));
    }
    match runtime {
        CURSOR_RUNTIME => cursor::provision(http, agent, credential, input).await,
        CLAUDE_AGENTS_RUNTIME => claude_agents::provision(http, agent, credential, input).await,
        _ => unreachable!(),
    }
}
