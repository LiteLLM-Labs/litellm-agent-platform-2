use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::managed_agents::registry::schema::ManagedAgentRow;

pub const CURSOR_RUNTIME: &str = "cursor";
pub const CLAUDE_AGENTS_RUNTIME: &str = "claude_agents";

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
    matches!(runtime, CURSOR_RUNTIME | CLAUDE_AGENTS_RUNTIME)
}

pub fn default_api_base(runtime: &str) -> Option<&'static str> {
    match runtime {
        CURSOR_RUNTIME => Some("https://api.cursor.com"),
        CLAUDE_AGENTS_RUNTIME => Some("https://api.anthropic.com"),
        _ => None,
    }
}

pub fn runtime_agent_id(agent: &ManagedAgentRow, runtime: &str) -> String {
    format!("{runtime}:{}", agent.id)
}
