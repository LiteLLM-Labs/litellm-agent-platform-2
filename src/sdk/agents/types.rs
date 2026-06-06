use std::{collections::HashMap, fmt};

use serde::Serialize;
use serde_json::Value;

pub const CLAUDE_MANAGED_AGENTS: &str = "claude_managed_agents";
pub const CURSOR: &str = "cursor";
pub const OPENCODE: &str = "opencode";
pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CURSOR_BASE_URL: &str = "https://api.cursor.com";
pub const DEFAULT_OPENCODE_BASE_URL: &str = "http://127.0.0.1:4096";
pub const MANAGED_AGENTS_BETA: &str = "managed-agents-2026-04-01";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRuntime {
    ClaudeManagedAgents,
    Cursor,
    OpenCode,
}

impl AgentRuntime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeManagedAgents => CLAUDE_MANAGED_AGENTS,
            Self::Cursor => CURSOR,
            Self::OpenCode => OPENCODE,
        }
    }
}

impl TryFrom<&str> for AgentRuntime {
    type Error = AgentSdkError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            CLAUDE_MANAGED_AGENTS => Ok(Self::ClaudeManagedAgents),
            CURSOR => Ok(Self::Cursor),
            OPENCODE => Ok(Self::OpenCode),
            runtime => Err(AgentSdkError::UnsupportedRuntime(runtime.to_owned())),
        }
    }
}

impl fmt::Display for AgentRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct LapConfig {
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: String,
    pub cursor_api_key: Option<String>,
    pub cursor_base_url: String,
    pub opencode_base_url: Option<String>,
    pub opencode_username: String,
    pub opencode_password: Option<String>,
}

impl LapConfig {
    pub fn anthropic(api_key: impl Into<String>) -> Self {
        Self {
            anthropic_api_key: Some(api_key.into()),
            ..Self::default()
        }
    }

    pub fn cursor(api_key: impl Into<String>) -> Self {
        Self {
            cursor_api_key: Some(api_key.into()),
            ..Self::default()
        }
    }

    pub fn opencode(base_url: impl Into<String>) -> Self {
        Self {
            opencode_base_url: Some(base_url.into()),
            ..Self::default()
        }
    }
}

impl Default for LapConfig {
    fn default() -> Self {
        Self {
            anthropic_api_key: None,
            anthropic_base_url: DEFAULT_ANTHROPIC_BASE_URL.to_owned(),
            cursor_api_key: None,
            cursor_base_url: DEFAULT_CURSOR_BASE_URL.to_owned(),
            opencode_base_url: None,
            opencode_username: "opencode".to_owned(),
            opencode_password: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAgentParams {
    #[serde(skip)]
    pub lap_agent_runtime: AgentRuntime,
    #[serde(skip)]
    pub lap_provider_options: Option<Value>,
    pub name: String,
    pub model: AgentModel,
    pub system: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum AgentModel {
    Id(String),
    Config(AgentModelConfig),
}

impl From<&str> for AgentModel {
    fn from(value: &str) -> Self {
        Self::Id(value.to_owned())
    }
}

impl From<String> for AgentModel {
    fn from(value: String) -> Self {
        Self::Id(value)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentModelConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateEnvironmentParams {
    #[serde(skip)]
    pub lap_agent_runtime: AgentRuntime,
    pub name: String,
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionParams {
    pub agent: String,
    pub environment_id: String,
    pub title: String,
    #[serde(skip)]
    pub lap_agent_runtime: Option<AgentRuntime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
}

impl CreateSessionParams {
    pub fn opencode(title: impl Into<String>) -> Self {
        Self {
            agent: String::new(),
            environment_id: String::new(),
            title: title.into(),
            lap_agent_runtime: Some(AgentRuntime::OpenCode),
            metadata: None,
            resources: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SendEventsParams {
    pub events: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct ManagedSessionRef {
    pub session_id: String,
    pub lap_agent_runtime: AgentRuntime,
    pub provider_session_id: Option<String>,
    pub provider_agent_id: Option<String>,
    pub provider_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagedAgent {
    pub id: String,
    pub version: Option<u64>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    pub id: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendEventsResponse {
    pub raw: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentSdkError {
    #[error("unsupported lap_agent_runtime: {0}")]
    UnsupportedRuntime(String),
    #[error("no agent runtimes configured")]
    NoRuntimesConfigured,
    #[error("lap_agent_runtime is required when multiple runtimes are configured")]
    RuntimeRequired,
    #[error("{0} runtime is not configured")]
    RuntimeNotConfigured(AgentRuntime),
    #[error("provider request failed with status {status}: {body}")]
    Provider {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("provider response is missing id")]
    MissingId,
    #[error("provider response is missing {0}")]
    MissingField(&'static str),
    #[error("invalid managed agent SDK request: {0}")]
    InvalidRequest(String),
    #[error("managed agent SDK state lock failed")]
    StateLock,
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}
