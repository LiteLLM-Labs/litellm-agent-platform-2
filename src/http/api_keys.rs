use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    errors::GatewayError,
    proxy::{
        auth::{
            api_keys::{ApiKeyEntry, CreatedApiKey},
            master_key::{presented_key, require_master_key},
        },
        state::AppState,
    },
};

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListApiKeysResponse {
    pub keys: Vec<ApiKeyEntry>,
}

#[derive(Debug, Serialize)]
pub struct DeleteApiKeyResponse {
    pub ok: bool,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ListApiKeysResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    Ok(Json(ListApiKeysResponse {
        keys: state.api_keys.list().await,
    }))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreatedApiKey>), GatewayError> {
    require_admin(&state, &headers)?;
    Ok((
        StatusCode::CREATED,
        Json(state.api_keys.create(input.label).await),
    ))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Result<Json<ApiKeyEntry>, GatewayError> {
    require_admin(&state, &headers)?;
    let key = state
        .api_keys
        .get(&key_id)
        .await
        .ok_or_else(|| GatewayError::NotFound("api key not found".to_owned()))?;
    Ok(Json(key))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Json(input): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyEntry>, GatewayError> {
    require_admin(&state, &headers)?;
    let key = state
        .api_keys
        .update(&key_id, input.label)
        .await
        .ok_or_else(|| GatewayError::NotFound("api key not found".to_owned()))?;
    Ok(Json(key))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
) -> Result<Json<DeleteApiKeyResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    Ok(Json(DeleteApiKeyResponse {
        ok: state.api_keys.delete(&key_id).await,
    }))
}

pub async fn require_gateway_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), GatewayError> {
    if require_master_key(headers, state.config.general_settings.master_key.as_deref()).is_ok() {
        return Ok(());
    }

    if let Some(key) = presented_key(headers) {
        if state.api_keys.authenticate(key).await {
            return Ok(());
        }
    }

    Err(GatewayError::Unauthorized)
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), GatewayError> {
    require_master_key(headers, state.config.general_settings.master_key.as_deref())
}
