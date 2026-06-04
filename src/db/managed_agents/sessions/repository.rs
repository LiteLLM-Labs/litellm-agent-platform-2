use sqlx::PgPool;

use crate::{
    db::managed_agents::{id, now_ms},
    errors::GatewayError,
};

use super::schema::SessionRow;

pub async fn create(
    pool: &PgPool,
    harness: &str,
    agent_id: Option<&str>,
    title: &str,
    timezone: Option<&str>,
) -> Result<SessionRow, GatewayError> {
    let session_id = id("ses");
    sqlx::query_as::<_, SessionRow>(
        r#"
        INSERT INTO "LiteLLM_ManagedAgentSessionsTable"
          (id, harness, agent_id, title, created_at, tz)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
    )
    .bind(session_id)
    .bind(harness)
    .bind(agent_id)
    .bind(title)
    .bind(now_ms())
    .bind(timezone)
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn list(pool: &PgPool) -> Result<Vec<SessionRow>, GatewayError> {
    sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT *
        FROM "LiteLLM_ManagedAgentSessionsTable"
        ORDER BY COALESCE(updated_at, created_at) DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn get(pool: &PgPool, session_id: &str) -> Result<Option<SessionRow>, GatewayError> {
    sqlx::query_as::<_, SessionRow>(
        r#"SELECT * FROM "LiteLLM_ManagedAgentSessionsTable" WHERE id = $1"#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn delete(pool: &PgPool, session_id: &str) -> Result<bool, GatewayError> {
    let result = sqlx::query(r#"DELETE FROM "LiteLLM_ManagedAgentSessionsTable" WHERE id = $1"#)
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(GatewayError::Database)?;
    Ok(result.rows_affected() > 0)
}

pub async fn touch(pool: &PgPool, session_id: &str) -> Result<(), GatewayError> {
    sqlx::query(
        r#"
        UPDATE "LiteLLM_ManagedAgentSessionsTable"
        SET updated_at = $2
        WHERE id = $1
        "#,
    )
    .bind(session_id)
    .bind(now_ms())
    .execute(pool)
    .await
    .map_err(GatewayError::Database)?;
    Ok(())
}
