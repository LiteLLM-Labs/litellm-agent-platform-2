use std::{collections::HashMap, fmt};

use serde::Serialize;
use serde_json::Value;

#[path = "types_error.rs"]
mod errors;
pub use errors::AgentSdkError;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentRuntimeCatalogEntry {
    pub runtime: AgentRuntime,
    pub id: &'static str,
    pub name: &'static str,
    pub default_api_base: &'static str,
}

impl AgentRuntime {
    pub const CATALOG: [AgentRuntimeCatalogEntry; 3] = [
        AgentRuntimeCatalogEntry {
            runtime: Self::ClaudeManagedAgents,
            id: CLAUDE_MANAGED_AGENTS,
            name: "Claude Agents",
            default_api_base: DEFAULT_ANTHROPIC_BASE_URL,
        },
        AgentRuntimeCatalogEntry {
            runtime: Self::Cursor,
            id: CURSOR,
            name: "Cursor",
            default_api_base: DEFAULT_CURSOR_BASE_URL,
        },
        AgentRuntimeCatalogEntry {
            runtime: Self::OpenCode,
            id: OPENCODE,
            name: "OpenCode",
            default_api_base: DEFAULT_OPENCODE_BASE_URL,
        },
    ];

    pub fn catalog() -> &'static [AgentRuntimeCatalogEntry] {
        &Self::CATALOG
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeManagedAgents => CLAUDE_MANAGED_AGENTS,
            Self::Cursor => CURSOR,
            Self::OpenCode => OPENCODE,
        }
    }

    pub fn name(self) -> &'static str {
        Self::catalog()
            .iter()
            .find(|entry| entry.runtime == self)
            .map(|entry| entry.name)
            .unwrap_or_else(|| self.as_str())
    }

    pub fn default_api_base(self) -> &'static str {
        Self::catalog()
            .iter()
            .find(|entry| entry.runtime == self)
            .map(|entry| entry.default_api_base)
            .unwrap_or_default()
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
    pub opencode_api_key: Option<String>,
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
            opencode_api_key: None,
            opencode_base_url: None,
            opencode_username: "opencode".to_owned(),
            opencode_password: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentWorkspace {
    pub repository: String,
    pub ref_name: Option<String>,
    pub auto_create_pr: bool,
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
    #[serde(skip)]
    pub env_vars: Option<HashMap<String, String>>,
    #[serde(skip)]
    pub workspace: Option<AgentWorkspace>,
    #[serde(skip)]
    pub metadata: Option<HashMap<String, String>>,
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
    pub vault_ids: Option<Vec<String>>,
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
            vault_ids: None,
            resources: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SendEventsParams {
    pub events: Vec<Value>,
    /// Model the agent should run for this turn. Carried alongside the events
    /// for runtimes whose message API takes the model in the request body
    /// (e.g. opencode); `skip`ped from serialization so runtimes that post
    /// params verbatim never receive an unexpected field.
    #[serde(skip)]
    pub model: Option<String>,
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
    pub name: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub system: Option<String>,
    pub tools: Vec<Value>,
    pub mcp_servers: Vec<Value>,
    pub metadata: Option<Value>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
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
    pub agent: Option<String>,
    pub environment_id: Option<String>,
    pub status: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub raw: Value,
}

#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq)] pub struct SendEventsResponse { pub raw: Value }
