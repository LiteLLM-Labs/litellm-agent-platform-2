use sqlx::PgPool;

use crate::{
    db::managed_agents::{id, now_ms},
    errors::GatewayError,
};

use super::schema::AgentChannelRow;

/// Insert or update a channel row. On conflict (agent_id, kind) update config/status/updated_at.
pub async fn upsert(
    pool: &PgPool,
    agent_id: &str,
    kind: &str,
    config: serde_json::Value,
) -> Result<AgentChannelRow, GatewayError> {
    let now = now_ms();
    sqlx::query_as::<_, AgentChannelRow>(
        r#"
        INSERT INTO "LiteLLM_ManagedAgentChannelsTable"
          (id, agent_id, kind, status, config, created_at, updated_at)
        VALUES ($1, $2, $3, 'enabled', $4, $5, $6)
        ON CONFLICT (agent_id, kind)
        DO UPDATE SET config = EXCLUDED.config, updated_at = EXCLUDED.updated_at
        RETURNING *
        "#,
    )
    .bind(id("chan"))
    .bind(agent_id)
    .bind(kind)
    .bind(config)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}

/// Fetch the channel row for a given agent+kind, or None if not yet created.
pub async fn get_by_kind(
    pool: &PgPool,
    agent_id: &str,
    kind: &str,
) -> Result<Option<AgentChannelRow>, GatewayError> {
    sqlx::query_as::<_, AgentChannelRow>(
        r#"
        SELECT *
        FROM "LiteLLM_ManagedAgentChannelsTable"
        WHERE agent_id = $1 AND kind = $2
        "#,
    )
    .bind(agent_id)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

/// Partially update the config JSONB (merge patch — top-level keys only).
pub async fn update_config(
    pool: &PgPool,
    agent_id: &str,
    kind: &str,
    patch: serde_json::Value,
) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        UPDATE "LiteLLM_ManagedAgentChannelsTable"
        SET config = config || $1::jsonb, updated_at = $2
        WHERE agent_id = $3 AND kind = $4
        "#,
    )
    .bind(patch)
    .bind(now_ms())
    .bind(agent_id)
    .bind(kind)
    .execute(pool)
    .await
    .map_err(GatewayError::Database)?;
    Ok(())
}

pub async fn exists(pool: &PgPool, agent_id: &str, kind: &str) -> Result<bool, GatewayError> {
    sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS(SELECT 1 FROM "LiteLLM_ManagedAgentChannelsTable" WHERE agent_id = $1 AND kind = $2)"#,
    )
    .bind(agent_id)
    .bind(kind)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}
