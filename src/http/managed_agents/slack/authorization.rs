use crate::db::managed_agents::channels::{
    repository as channels_repo, schema::SlackChannelConfig,
};
use sqlx::PgPool;

pub async fn authorize_slack_invocation(
    pool: &PgPool,
    agent_id: &str,
    user_id: &str,
    team_id: &str,
) -> Result<(), ()> {
    let Ok(Some(channel)) = channels_repo::get_by_kind(pool, agent_id, "slack").await else {
        return Err(());
    };

    if channel.status != "enabled" {
        return Err(());
    }

    let config: SlackChannelConfig = serde_json::from_value(channel.config).unwrap_or_default();

    if let Some(ref stored_team) = config.team_id {
        if stored_team != team_id {
            return Err(());
        }
    }

    match config.access.as_deref().unwrap_or("only_me") {
        "everyone" => Ok(()),
        _ => {
            if config
                .allowed_user_ids
                .unwrap_or_default()
                .iter()
                .any(|id| id == user_id)
            {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}
