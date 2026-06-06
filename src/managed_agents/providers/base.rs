use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    sdk::agents::{AgentRuntime, CLAUDE_MANAGED_AGENTS, CURSOR, OPENCODE},
};

pub const CURSOR_RUNTIME: &str = CURSOR;
pub const CLAUDE_AGENTS_RUNTIME: &str = CLAUDE_MANAGED_AGENTS;
pub const CLAUDE_AGENTS_RUNTIME_LEGACY: &str = "claude_agents";

#[derive(Debug, Clone)]
pub struct RuntimeCredential {
    pub api_key: String,
    pub api_base: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeSessionInput {
    pub session_id: String,
    pub prompt: String,
    pub environment: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProvision {
    pub runtime_agent_id: String,
    pub provider_session_id: Option<String>,
    pub provider_run_id: Option<String>,
    pub provider_url: Option<String>,
    pub metadata: Value,
}

pub fn validate_runtime(runtime: &str) -> bool {
    normalize_runtime(runtime).is_some()
}

pub fn normalize_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        CLAUDE_AGENTS_RUNTIME | CLAUDE_AGENTS_RUNTIME_LEGACY => Some(CLAUDE_AGENTS_RUNTIME),
        CURSOR_RUNTIME => Some(CURSOR_RUNTIME),
        OPENCODE => Some(OPENCODE),
        _ => None,
    }
}

pub fn default_api_base(runtime: &str) -> Option<&'static str> {
    AgentRuntime::try_from(normalize_runtime(runtime)?)
        .ok()
        .map(AgentRuntime::default_api_base)
}

pub fn runtime_agent_id(agent: &ManagedAgentRow, runtime: &str) -> String {
    format!("{runtime}:{}", agent.id)
}
