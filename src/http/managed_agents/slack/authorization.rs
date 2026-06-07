use sqlx::PgPool;
use crate::{
    db::managed_agents::channels::{repository as channels_repo, schema::SlackChannelConfig},
    errors::GatewayError,
};

#[derive(Debug)]
pub enum SlackAuthError {
    NoChannel,
    ChannelDisabled,
    TeamMismatch,
    NotAllowed,
}

impl From<SlackAuthError> for GatewayError {
    fn from(e: SlackAuthError) -> Self {
        match e {
            SlackAuthError::NoChannel => GatewayError::Unauthorized,
            SlackAuthError::ChannelDisabled => GatewayError::Unauthorized,
            SlackAuthError::TeamMismatch => GatewayError::Unauthorized,
            SlackAuthError::NotAllowed => GatewayError::Unauthorized,
        }
    }
}

/// Check whether a Slack user may invoke an agent.
/// Returns Ok(()) if allowed, Err(SlackAuthError) if denied.
pub async fn authorize_slack_invocation(
    pool: &PgPool,
    agent_id: &str,
    user_id: &str,
    team_id: &str,
) -> Result<(), SlackAuthError> {
    let channel = channels_repo::get_by_kind(pool, agent_id, "slack")
        .await
        .map_err(|_| SlackAuthError::NoChannel)?
        .ok_or(SlackAuthError::NoChannel)?;

    if channel.status != "enabled" {
        return Err(SlackAuthError::ChannelDisabled);
    }

    let config: SlackChannelConfig = serde_json::from_value(channel.config)
        .unwrap_or_default();

    if let Some(ref stored_team) = config.team_id {
        if stored_team != team_id {
            return Err(SlackAuthError::TeamMismatch);
        }
    }

    match config.access.as_deref().unwrap_or("only_me") {
        "everyone" => Ok(()),
        _ => {
            let allowed = config.allowed_user_ids.unwrap_or_default();
            if allowed.iter().any(|id| id == user_id) {
                Ok(())
            } else {
                Err(SlackAuthError::NotAllowed)
            }
        }
    }
}
