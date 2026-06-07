use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct AgentChannelRow {
    pub id: String,
    pub agent_id: String,
    pub kind: String,
    pub status: String,
    pub config: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Deserialised view of config JSONB for a Slack channel.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SlackChannelConfig {
    pub team_id: Option<String>,
    /// "only_me" | "selected_users" | "everyone"
    pub access: Option<String>,
    pub allowed_user_ids: Option<Vec<String>>,
    /// Slack user_id of the person who completed OAuth — seeded as the first allowed user.
    pub authed_user_id: Option<String>,
}
