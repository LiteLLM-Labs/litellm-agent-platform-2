use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    db::managed_agents::{messages, sessions::schema::SessionRow},
    errors::GatewayError,
};

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub(crate) title: Option<String>,
    pub(crate) harness: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) runtime: Option<String>,
    pub(crate) prompt: Option<String>,
    pub(crate) environment: Option<Value>,
    pub(crate) timezone: Option<String>,
    pub(crate) tz: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    model: Option<PromptModel>,
    parts: Option<Vec<PromptPart>>,
}

impl PromptRequest {
    pub(crate) fn model_id(&self) -> Option<String> {
        self.model.as_ref().map(|model| model.model_id.clone())
    }

    pub(crate) fn prompt_text(&self) -> Result<String, GatewayError> {
        let text = self
            .parts
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|part| match part {
                PromptPart::Text { text } => Some(text.as_str()),
                PromptPart::Other => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            return Err(GatewayError::InvalidJsonMessage(
                "prompt text is required".to_owned(),
            ));
        }
        Ok(text)
    }
}

#[derive(Debug, Deserialize)]
struct PromptModel {
    #[serde(rename = "modelID")]
    model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PromptPart {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    id: String,
    title: String,
    agent: String,
    agent_id: Option<String>,
    harness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_agent_ref_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_run_id: Option<String>,
    status: String,
    environment: Value,
    time: SessionTime,
}

impl From<SessionRow> for SessionResponse {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            agent: row.agent_id.clone().unwrap_or_else(|| row.harness.clone()),
            agent_id: row.agent_id,
            harness: row.harness,
            runtime: row.runtime,
            runtime_agent_ref_id: row.runtime_agent_ref_id,
            provider_session_id: row.provider_session_id,
            provider_run_id: row.provider_run_id,
            status: row.status,
            environment: row.environment_json,
            time: SessionTime {
                created: row.created_at,
                updated: row.updated_at,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct SessionTime {
    created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    info: Value,
    parts: Value,
}

impl TryFrom<messages::schema::SessionMessageRow> for MessageResponse {
    type Error = GatewayError;

    fn try_from(row: messages::schema::SessionMessageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            info: serde_json::from_str(&row.info_json)?,
            parts: serde_json::from_str(&row.parts_json)?,
        })
    }
}
