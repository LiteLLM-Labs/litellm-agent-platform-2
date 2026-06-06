use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, state::AppState, vault},
};

#[derive(Debug, Deserialize)]
pub struct SaveVaultKeyRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct VaultKeysResponse {
    pub keys: Vec<vault::VaultKeyEntry>,
}

#[derive(Debug, Serialize)]
pub struct VaultSaveResponse {
    pub ok: bool,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<VaultKeysResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    Ok(Json(VaultKeysResponse {
        keys: vault::list(pool, &user_id).await?,
    }))
}

pub async fn save(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(input): Json<SaveVaultKeyRequest>,
) -> Result<Json<VaultSaveResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    vault::save(
        pool,
        &state.config,
        &user_id,
        input.key.trim(),
        &input.value,
    )
    .await?;
    Ok(Json(VaultSaveResponse { ok: true }))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key)): Path<(String, String)>,
) -> Result<(StatusCode, Json<VaultSaveResponse>), GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let ok = vault::delete(pool, &user_id, &key).await?;
    Ok((StatusCode::OK, Json(VaultSaveResponse { ok })))
}
