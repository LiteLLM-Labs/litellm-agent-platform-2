use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    db::{credentials, mcp_servers::repository},
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, credential_crypto, state::AppState},
};

use super::caller_user_id;

fn key_name(server_id: &str, user_id: &str) -> String {
    format!("mcp_user:{}:{}", server_id, user_id)
}

// ── request / response types ──────────────────────────────────────────────────

/// Body for POST /v1/mcp/server/{server_id}/user-credential
///
/// Accepts either `{ "credential": "..." }` or `{ "api_key": "..." }`.
#[derive(Debug, Deserialize)]
pub struct SaveUserCredentialRequest {
    pub credential: Option<String>,
    pub api_key: Option<String>,
}

impl SaveUserCredentialRequest {
    fn value(&self) -> Option<&str> {
        self.credential.as_deref().or(self.api_key.as_deref())
    }
}

#[derive(Debug, Serialize)]
pub struct SaveUserCredentialResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct DeleteUserCredentialResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct UserCredentialEntry {
    pub server_id: String,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListUserCredentialsResponse {
    pub data: Vec<UserCredentialEntry>,
}

// ── handlers ──────────────────────────────────────────────────────────────────

/// POST /v1/mcp/server/{server_id}/user-credential
///
/// Store (or replace) the caller's personal credential for a BYOK MCP server.
pub async fn store(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(input): Json<SaveUserCredentialRequest>,
) -> Result<Json<SaveUserCredentialResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;

    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;

    // Validate the server exists in the DB registry.
    let server_exists = repository::get(pool, &server_id).await?.is_some();
    if !server_exists {
        return Err(GatewayError::UnknownMcpServer(server_id));
    }

    let raw_value = input
        .value()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidJsonMessage("credential or api_key is required".to_owned())
        })?;

    let user_id = caller_user_id(&headers, &state);
    let enc_key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    let encrypted = credential_crypto::encrypt_value(raw_value.trim(), &enc_key)?;

    let k = key_name(&server_id, &user_id);
    credentials::upsert_vault_key(pool, &k, "personal", Some(&user_id), &encrypted, &user_id)
        .await?;

    Ok(Json(SaveUserCredentialResponse { ok: true }))
}

/// DELETE /v1/mcp/server/{server_id}/user-credential
///
/// Remove the caller's personal credential for a BYOK MCP server.
/// Returns 200 if deleted, 404 if no credential was found.
pub async fn delete_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<(StatusCode, Json<DeleteUserCredentialResponse>), GatewayError> {
    require_any_gateway_key(&headers, &state)?;

    let user_id = caller_user_id(&headers, &state);

    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let k = key_name(&server_id, &user_id);
    let deleted = credentials::delete_vault_key(pool, &k, "personal", Some(&user_id)).await?;

    let status = if deleted {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    Ok((status, Json(DeleteUserCredentialResponse { ok: deleted })))
}

/// GET /v1/mcp/user-credentials
///
/// List all MCP server credentials that belong to the calling user.
/// Returns metadata only — never the encrypted value.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ListUserCredentialsResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;

    let user_id = caller_user_id(&headers, &state);

    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;

    let rows = sqlx::query_as::<_, credentials::VaultKeyRow>(
        r#"
        SELECT
            credential_name,
            scope,
            owner_id,
            CAST(EXTRACT(EPOCH FROM updated_at) * 1000 AS BIGINT) AS updated_at_ms
        FROM "LiteLLM_CredentialsTable"
        WHERE owner_id = $1
          AND credential_name LIKE 'mcp_user:%'
          AND scope = 'personal'
        ORDER BY credential_name ASC
        "#,
    )
    .bind(&user_id)
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)?;

    let data = rows
        .into_iter()
        .map(|r| {
            // key format: mcp_user:{server_id}:{user_id}
            // The user_id portion may itself contain ':', so we only split at
            // the first two colons and take the middle part as server_id.
            let parts: Vec<&str> = r.credential_name.splitn(3, ':').collect();
            let server_id = parts.get(1).copied().unwrap_or("").to_owned();
            UserCredentialEntry {
                server_id,
                updated_at: r.updated_at_ms,
            }
        })
        .collect();

    Ok(Json(ListUserCredentialsResponse { data }))
}
