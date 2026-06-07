use sqlx::PgPool;

use crate::{
    db::managed_agents::{id, now_ms},
    errors::GatewayError,
};

use super::schema::{SlackAgentBindingRow, SlackPendingInstallRow};

pub async fn upsert_binding(
    pool: &PgPool,
    platform_agent_id: &str,
    agent_id: &str,
    team_id: Option<&str>,
    channel_id: &str,
    dm_user_id: Option<&str>,
    created_by: Option<&str>,
) -> Result<SlackAgentBindingRow, GatewayError> {
    let now = now_ms();
    let input = BindingInput {
        agent_id,
        team_id,
        channel_id,
        dm_user_id,
        created_by,
        now,
    };
    if let Some(row) = update_binding(pool, platform_agent_id, &input).await? {
        return Ok(row);
    }
    sqlx::query_as::<_, SlackAgentBindingRow>(
        r#"
        INSERT INTO "LiteLLM_SlackAgentBindingsTable"
          (id, platform_agent_id, agent_id, team_id, channel_id, dm_user_id, created_by, status, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'connected', $8, $8)
        RETURNING *
        "#,
    )
    .bind(id("slack_binding"))
    .bind(platform_agent_id)
    .bind(input.agent_id)
    .bind(input.team_id)
    .bind(input.channel_id)
    .bind(input.dm_user_id)
    .bind(input.created_by)
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}

struct BindingInput<'a> {
    agent_id: &'a str,
    team_id: Option<&'a str>,
    channel_id: &'a str,
    dm_user_id: Option<&'a str>,
    created_by: Option<&'a str>,
    now: i64,
}

async fn update_binding(
    pool: &PgPool,
    platform_agent_id: &str,
    input: &BindingInput<'_>,
) -> Result<Option<SlackAgentBindingRow>, GatewayError> {
    sqlx::query_as::<_, SlackAgentBindingRow>(
        r#"
        UPDATE "LiteLLM_SlackAgentBindingsTable"
        SET agent_id = $4,
            dm_user_id = $5,
            created_by = COALESCE($6, created_by),
            status = 'connected',
            updated_at = $7
        WHERE platform_agent_id = $1
          AND channel_id = $2
          AND team_id IS NOT DISTINCT FROM $3
        RETURNING *
        "#,
    )
    .bind(platform_agent_id)
    .bind(input.channel_id)
    .bind(input.team_id)
    .bind(input.agent_id)
    .bind(input.dm_user_id)
    .bind(input.created_by)
    .bind(input.now)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn get_binding(
    pool: &PgPool,
    platform_agent_id: &str,
    team_id: Option<&str>,
    channel_id: &str,
) -> Result<Option<SlackAgentBindingRow>, GatewayError> {
    sqlx::query_as::<_, SlackAgentBindingRow>(
        r#"
        SELECT *
        FROM "LiteLLM_SlackAgentBindingsTable"
        WHERE platform_agent_id = $1
          AND channel_id = $2
          AND team_id IS NOT DISTINCT FROM $3
          AND status = 'connected'
        "#,
    )
    .bind(platform_agent_id)
    .bind(channel_id)
    .bind(team_id)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn list_bindings(
    pool: &PgPool,
    platform_agent_id: &str,
) -> Result<Vec<SlackAgentBindingRow>, GatewayError> {
    sqlx::query_as::<_, SlackAgentBindingRow>(
        r#"
        SELECT *
        FROM "LiteLLM_SlackAgentBindingsTable"
        WHERE platform_agent_id = $1
        ORDER BY updated_at DESC
        "#,
    )
    .bind(platform_agent_id)
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn create_pending_install(
    pool: &PgPool,
    input: PendingInstallInput<'_>,
) -> Result<SlackPendingInstallRow, GatewayError> {
    let now = now_ms();
    sqlx::query_as::<_, SlackPendingInstallRow>(
        r#"
        INSERT INTO "LiteLLM_SlackPendingInstallsTable"
          (state, platform_agent_id, agent_id, team_id, channel_id, dm_user_id, requested_by, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        "#,
    )
    .bind(input.state)
    .bind(input.platform_agent_id)
    .bind(input.agent_id)
    .bind(input.team_id)
    .bind(input.channel_id)
    .bind(input.dm_user_id)
    .bind(input.requested_by)
    .bind(now)
    .bind(now + 10 * 60 * 1000)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}

pub struct PendingInstallInput<'a> {
    pub state: &'a str,
    pub platform_agent_id: &'a str,
    pub agent_id: &'a str,
    pub team_id: Option<&'a str>,
    pub channel_id: &'a str,
    pub dm_user_id: Option<&'a str>,
    pub requested_by: Option<&'a str>,
}

pub async fn consume_pending_install(
    pool: &PgPool,
    state: &str,
) -> Result<Option<SlackPendingInstallRow>, GatewayError> {
    sqlx::query_as::<_, SlackPendingInstallRow>(
        r#"
        UPDATE "LiteLLM_SlackPendingInstallsTable"
        SET used_at = $2
        WHERE state = $1 AND used_at IS NULL AND expires_at >= $2
        RETURNING *
        "#,
    )
    .bind(state)
    .bind(now_ms())
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}
