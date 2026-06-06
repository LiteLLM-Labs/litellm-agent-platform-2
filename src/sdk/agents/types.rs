use std::{collections::HashMap, fmt};

use serde::Serialize;
use serde_json::Value;

pub const CLAUDE_MANAGED_AGENTS: &str = "claude_managed_agents";
pub const CURSOR: &str = "cursor";
pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CURSOR_BASE_URL: &str = "https://api.cursor.com";
pub const MANAGED_AGENTS_BETA: &str = "managed-agents-2026-04-01";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRuntime {
    ClaudeManagedAgents,
    Cursor,
}

impl AgentRuntime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeManagedAgents => CLAUDE_MANAGED_AGENTS,
            Self::Cursor => CURSOR,
        }
    }
}

impl TryFrom<&str> for AgentRuntime {
    type Error = AgentSdkError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            CLAUDE_MANAGED_AGENTS => Ok(Self::ClaudeManagedAgents),
            CURSOR => Ok(Self::Cursor),
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
}

impl Default for LapConfig {
    fn default() -> Self {
        Self {
            anthropic_api_key: None,
            anthropic_base_url: DEFAULT_ANTHROPIC_BASE_URL.to_owned(),
            cursor_api_key: None,
            cursor_base_url: DEFAULT_CURSOR_BASE_URL.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAgentParams {
    #[serde(skip)]
    pub lap_agent_runtime: AgentRuntime,
    pub name: String,
    pub model: AgentModel,
    pub system: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ManagedAgentTool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<ManagedAgentMcpServer>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
#[serde(tag = "type")]
pub enum ManagedAgentTool {
    #[serde(rename = "agent_toolset_20260401")]
    AgentToolset20260401,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedAgentMcpServer {
    pub name: String,
    #[serde(rename = "type")]
    pub server_type: ManagedAgentMcpServerType,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum ManagedAgentMcpServerType {
    #[serde(rename = "url")]
    Url,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateEnvironmentParams {
    #[serde(skip)]
    pub lap_agent_runtime: AgentRuntime,
    pub name: String,
    pub config: EnvironmentConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum EnvironmentConfig {
    #[serde(rename = "cloud")]
    Cloud { networking: EnvironmentNetworking },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum EnvironmentNetworking {
    #[serde(rename = "unrestricted")]
    Unrestricted,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionParams {
    pub agent: SessionAgentReference,
    pub environment_id: String,
    pub title: String,
    #[serde(skip)]
    pub lap_agent_runtime: Option<AgentRuntime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SessionAgentReference {
    Id(String),
    Versioned(VersionedAgentReference),
}

impl SessionAgentReference {
    pub fn id(&self) -> &str {
        match self {
            Self::Id(id) => id,
            Self::Versioned(reference) => &reference.id,
        }
    }
}

impl From<&str> for SessionAgentReference {
    fn from(value: &str) -> Self {
        Self::Id(value.to_owned())
    }
}

impl From<String> for SessionAgentReference {
    fn from(value: String) -> Self {
        Self::Id(value)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionedAgentReference {
    pub id: String,
    #[serde(rename = "type")]
    pub reference_type: AgentReferenceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub enum AgentReferenceType {
    #[serde(rename = "agent")]
    Agent,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendEventsParams {
    pub events: Vec<UserEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum UserEvent {
    #[serde(rename = "user.message")]
    Message { content: Vec<UserContentBlock> },
    #[serde(rename = "user.interrupt")]
    Interrupt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum UserContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<ImageSource>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageSource {
    pub data: String,
    pub mime_type: String,
}

impl UserEvent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Message {
            content: vec![UserContentBlock::Text { text: text.into() }],
        }
    }
}

impl UserContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image {
            url: Some(url.into()),
            source: None,
        }
    }

    pub fn image_base64(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            url: None,
            source: Some(ImageSource {
                data: data.into(),
                mime_type: mime_type.into(),
            }),
        }
    }
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
    #[error("invalid managed agent SDK request: {0}")]
    InvalidRequest(String),
    #[error("provider response is missing id")]
    MissingId,
    #[error("provider response is missing {0}")]
    MissingField(&'static str),
    #[error("managed agent SDK state lock failed")]
    StateLock,
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
}
