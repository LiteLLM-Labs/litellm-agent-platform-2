use sqlx::PgPool;

use crate::{db::managed_agents::now_ms, errors::GatewayError};

use super::schema::SlackThreadSessionRow;

pub async fn list(
    pool: &PgPool,
    agent_id: &str,
) -> Result<Vec<SlackThreadSessionRow>, GatewayError> {
    sqlx::query_as::<_, SlackThreadSessionRow>(
        r#"
        SELECT *
        FROM "LiteLLM_ManagedAgentSlackThreadSessionsTable"
        WHERE agent_id = $1
        ORDER BY updated_at DESC
        "#,
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn get(
    pool: &PgPool,
    agent_id: &str,
    channel_id: &str,
    thread_ts: &str,
) -> Result<Option<SlackThreadSessionRow>, GatewayError> {
    sqlx::query_as::<_, SlackThreadSessionRow>(
        r#"
        SELECT *
        FROM "LiteLLM_ManagedAgentSlackThreadSessionsTable"
        WHERE agent_id = $1 AND channel_id = $2 AND thread_ts = $3
        "#,
    )
    .bind(agent_id)
    .bind(channel_id)
    .bind(thread_ts)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn upsert(
    pool: &PgPool,
    agent_id: &str,
    channel_id: &str,
    thread_ts: &str,
    session_id: &str,
) -> Result<SlackThreadSessionRow, GatewayError> {
    let now = now_ms();
    sqlx::query_as::<_, SlackThreadSessionRow>(
        r#"
        INSERT INTO "LiteLLM_ManagedAgentSlackThreadSessionsTable"
          (agent_id, channel_id, thread_ts, session_id, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $5)
        ON CONFLICT (agent_id, channel_id, thread_ts) DO UPDATE SET
          session_id = EXCLUDED.session_id,
          updated_at = EXCLUDED.updated_at
        RETURNING *
        "#,
    )
    .bind(agent_id)
    .bind(channel_id)
    .bind(thread_ts)
    .bind(session_id)
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}
