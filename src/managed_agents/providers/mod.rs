pub mod base;
pub mod claude_agents;
pub mod cursor;
pub mod opencode;

use crate::{db::managed_agents::registry::schema::ManagedAgentRow, errors::GatewayError};

use self::base::{
    normalize_runtime, RuntimeCredential, RuntimeProvision, RuntimeSessionInput, RuntimeTool,
    CLAUDE_AGENTS_RUNTIME, CURSOR_RUNTIME,
};

pub async fn provision_runtime(
    http: &reqwest::Client,
    runtime: &str,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let Some(runtime) = normalize_runtime(runtime) else {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported runtime: {runtime}"
        )));
    };
    match runtime {
        CURSOR_RUNTIME => cursor::provision(http, agent, credential, input).await,
        CLAUDE_AGENTS_RUNTIME => claude_agents::provision(http, agent, credential, input).await,
        crate::sdk::agents::OPENCODE => opencode::provision(http, agent, credential, input).await,
        _ => unreachable!(),
    }
}

pub fn runtime_tools(runtime: &str) -> Option<&'static [RuntimeTool]> {
    match normalize_runtime(runtime)? {
        CURSOR_RUNTIME => Some(cursor::TOOLS),
        CLAUDE_AGENTS_RUNTIME => Some(claude_agents::TOOLS),
        crate::sdk::agents::OPENCODE => Some(opencode::TOOLS),
        _ => unreachable!(),
    }
}
