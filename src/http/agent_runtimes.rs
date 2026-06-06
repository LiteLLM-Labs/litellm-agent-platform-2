use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    db::credentials,
    errors::GatewayError,
    proxy::{auth::master_key::require_master_key, credential_crypto, state::AppState},
    sdk::providers,
};

/// Opaque credential loaded from the DB for a runtime.
#[derive(Debug, Clone)]
pub struct RuntimeCredential {
    pub(crate) api_key: String,
    pub(crate) api_base: String,
}

#[derive(Debug, Serialize)]
pub struct AgentRuntimesResponse {
    pub runtimes: Vec<RuntimeResponse>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeResponse {
    pub id: String,
    pub name: String,
    pub default_api_base: String,
    pub connected: bool,
    pub api_base: Option<String>,
    pub masked_api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveRuntimeCredentialRequest {
    pub api_key: String,
    pub api_base: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeleteRuntimeCredentialResponse {
    pub ok: bool,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AgentRuntimesResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    Ok(Json(AgentRuntimesResponse {
        runtimes: runtime_values(&state).await?,
    }))
}

pub async fn save(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(runtime): Path<String>,
    Json(input): Json<SaveRuntimeCredentialRequest>,
) -> Result<Json<AgentRuntimesResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    validate(&runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let api_key = input.api_key.trim();
    if api_key.is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "api_key is required".to_owned(),
        ));
    }
    let registry = providers::runtime_registry();
    let api_base = input
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            registry
                .entry_for_id(&runtime)
                .map(|e| e.default_api_base)
                .unwrap_or_default()
        });
    let key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    credentials::upsert(
        pool,
        &credential_name(&runtime),
        json!({
            "api_key": credential_crypto::encrypt_value(api_key, &key)?,
            "api_base": credential_crypto::encrypt_value(api_base, &key)?,
        }),
        json!({ "runtime": runtime, "source": "agent-runtimes-ui" }),
        "ui",
    )
    .await?;
    Ok(Json(AgentRuntimesResponse {
        runtimes: runtime_values(&state).await?,
    }))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(runtime): Path<String>,
) -> Result<(StatusCode, Json<DeleteRuntimeCredentialResponse>), GatewayError> {
    require_admin(&state, &headers)?;
    validate(&runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    Ok((
        StatusCode::OK,
        Json(DeleteRuntimeCredentialResponse {
            ok: credentials::delete_by_name(pool, &credential_name(&runtime)).await?,
        }),
    ))
}

pub async fn load_credential(
    state: &AppState,
    runtime: &str,
) -> Result<RuntimeCredential, GatewayError> {
    validate(runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let Some(row) = credentials::get_by_name(pool, &credential_name(runtime)).await? else {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "{runtime} credentials are not configured"
        )));
    };
    let key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    let values = row.credential_values.as_object().ok_or_else(|| {
        GatewayError::InvalidConfig("runtime credential_values must be an object".to_owned())
    })?;
    let api_key = decrypt(values, "api_key", &key)?;
    let api_base = decrypt(values, "api_base", &key)?;
    Ok(RuntimeCredential { api_key, api_base })
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), GatewayError> {
    require_master_key(headers, state.config.general_settings.master_key.as_deref())
}

async fn runtime_values(state: &AppState) -> Result<Vec<RuntimeResponse>, GatewayError> {
    let registry = providers::runtime_registry();
    let mut values = Vec::new();
    for entry in registry.all_entries() {
        let credential = match load_credential(state, entry.id).await {
            Ok(value) => Some(value),
            Err(GatewayError::InvalidJsonMessage(_)) | Err(GatewayError::MissingDatabase) => None,
            Err(error) => return Err(error),
        };
        values.push(RuntimeResponse {
            id: entry.id.to_owned(),
            name: entry.name.to_owned(),
            default_api_base: entry.default_api_base.to_owned(),
            connected: credential.is_some(),
            api_base: credential.as_ref().map(|c| c.api_base.clone()),
            masked_api_key: credential.map(|c| mask(&c.api_key)),
        });
    }
    Ok(values)
}

fn credential_name(runtime: &str) -> String {
    format!("agent-runtime:{runtime}")
}

fn validate(runtime: &str) -> Result<(), GatewayError> {
    if providers::runtime_registry().validate_id(runtime) {
        Ok(())
    } else {
        Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported runtime: {runtime}"
        )))
    }
}

fn decrypt(
    values: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    key: &str,
) -> Result<String, GatewayError> {
    let encrypted = values
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| GatewayError::InvalidConfig(format!("credential is missing {field}")))?;
    credential_crypto::decrypt_value(encrypted, key)
}

fn mask(api_key: &str) -> String {
    let trimmed = api_key.trim();
    if trimmed.len() <= 12 {
        "Configured".to_owned()
    } else {
        format!("{}...{}", &trimmed[..7], &trimmed[trimmed.len() - 4..])
    }
}
