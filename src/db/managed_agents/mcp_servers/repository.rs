use sqlx::PgPool;

use crate::{
    db::managed_agents::{id, now_ms},
    errors::GatewayError,
};

use super::schema::{CreateManagedMcpServer, ManagedMcpServerRow, UpdateManagedMcpServer};

pub async fn create(
    pool: &PgPool,
    input: CreateManagedMcpServer,
) -> Result<ManagedMcpServerRow, GatewayError> {
    validate_required(&input.name, &input.url)?;
    let auth_type = normalized_auth_type(input.auth_type.as_deref())?;
    validate_auth_value(auth_type, input.auth_value.as_deref())?;

    sqlx::query_as::<_, ManagedMcpServerRow>(
        r#"
        INSERT INTO "LiteLLM_ManagedMcpServersTable"
          (id, name, url, auth_type, auth_value, description, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
    )
    .bind(id("mcp"))
    .bind(input.name)
    .bind(input.url)
    .bind(auth_type)
    .bind(input.auth_value)
    .bind(input.description)
    .bind(now_ms())
    .fetch_one(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn list(pool: &PgPool) -> Result<Vec<ManagedMcpServerRow>, GatewayError> {
    sqlx::query_as::<_, ManagedMcpServerRow>(
        r#"
        SELECT * FROM "LiteLLM_ManagedMcpServersTable"
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn get(
    pool: &PgPool,
    server_id: &str,
) -> Result<Option<ManagedMcpServerRow>, GatewayError> {
    sqlx::query_as::<_, ManagedMcpServerRow>(
        r#"SELECT * FROM "LiteLLM_ManagedMcpServersTable" WHERE id = $1"#,
    )
    .bind(server_id)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn update(
    pool: &PgPool,
    server_id: &str,
    input: UpdateManagedMcpServer,
) -> Result<Option<ManagedMcpServerRow>, GatewayError> {
    if let Some(name) = input.name.as_deref() {
        if name.trim().is_empty() {
            return Err(GatewayError::InvalidJsonMessage(
                "name cannot be empty".to_owned(),
            ));
        }
    }
    if let Some(url) = input.url.as_deref() {
        if url.trim().is_empty() {
            return Err(GatewayError::InvalidJsonMessage(
                "url cannot be empty".to_owned(),
            ));
        }
    }
    let auth_type = match input.auth_type.as_deref() {
        Some(value) => Some(normalized_auth_type(Some(value))?),
        None => None,
    };
    if let Some(auth_type) = auth_type {
        validate_auth_value(auth_type, input.auth_value.as_deref())?;
    }

    sqlx::query_as::<_, ManagedMcpServerRow>(
        r#"
        UPDATE "LiteLLM_ManagedMcpServersTable"
        SET
          name = COALESCE($2, name),
          url = COALESCE($3, url),
          auth_type = COALESCE($4, auth_type),
          auth_value = COALESCE($5, auth_value),
          description = COALESCE($6, description)
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(server_id)
    .bind(input.name)
    .bind(input.url)
    .bind(auth_type)
    .bind(input.auth_value)
    .bind(input.description)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)
}

pub async fn delete(pool: &PgPool, server_id: &str) -> Result<bool, GatewayError> {
    let result = sqlx::query(r#"DELETE FROM "LiteLLM_ManagedMcpServersTable" WHERE id = $1"#)
        .bind(server_id)
        .execute(pool)
        .await
        .map_err(GatewayError::Database)?;
    Ok(result.rows_affected() > 0)
}

fn validate_required(name: &str, url: &str) -> Result<(), GatewayError> {
    if name.trim().is_empty() || url.trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "name and url required".to_owned(),
        ));
    }
    Ok(())
}

fn normalized_auth_type(value: Option<&str>) -> Result<&'static str, GatewayError> {
    match value.unwrap_or("api_key") {
        "api_key" => Ok("api_key"),
        "bearer_token" => Ok("bearer_token"),
        "authorization" => Ok("authorization"),
        "none" => Ok("none"),
        other => Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported auth_type: {other}"
        ))),
    }
}

fn validate_auth_value(auth_type: &str, value: Option<&str>) -> Result<(), GatewayError> {
    if auth_type != "none" && value.unwrap_or("").trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "auth_value is required for key auth".to_owned(),
        ));
    }
    Ok(())
}
