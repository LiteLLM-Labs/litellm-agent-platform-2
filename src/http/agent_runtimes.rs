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
    managed_agents::providers::base::{
        default_api_base, normalize_runtime, RuntimeCredential, CLAUDE_AGENTS_RUNTIME,
        CLAUDE_AGENTS_RUNTIME_LEGACY,
    },
    proxy::{auth::master_key::require_master_key, credential_crypto, state::AppState},
    sdk::agents::{AgentRuntime, AgentRuntimeCatalogEntry},
};

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
    let runtime = canonical_runtime(&runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let api_key = input.api_key.trim();
    if api_key.is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "api_key is required".to_owned(),
        ));
    }
    let api_base = input
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_api_base(runtime).unwrap_or_default());
    let key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    credentials::upsert(
        pool,
        &credential_name(runtime),
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
    let runtime = canonical_runtime(&runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let deleted = credentials::delete_by_name(pool, &credential_name(runtime)).await?;
    let deleted_legacy = if runtime == CLAUDE_AGENTS_RUNTIME {
        credentials::delete_by_name(pool, &credential_name(CLAUDE_AGENTS_RUNTIME_LEGACY)).await?
    } else {
        false
    };
    Ok((
        StatusCode::OK,
        Json(DeleteRuntimeCredentialResponse {
            ok: deleted || deleted_legacy,
        }),
    ))
}

pub async fn load_credential(
    state: &AppState,
    runtime: &str,
) -> Result<RuntimeCredential, GatewayError> {
    let runtime = canonical_runtime(runtime)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let row = match credentials::get_by_name(pool, &credential_name(runtime)).await? {
        Some(row) => row,
        None if runtime == CLAUDE_AGENTS_RUNTIME => {
            match credentials::get_by_name(pool, &credential_name(CLAUDE_AGENTS_RUNTIME_LEGACY))
                .await?
            {
                Some(row) => row,
                None => {
                    return Err(GatewayError::InvalidJsonMessage(format!(
                        "{runtime} credentials are not configured"
                    )))
                }
            }
        }
        None => {
            return Err(GatewayError::InvalidJsonMessage(format!(
                "{runtime} credentials are not configured"
            )))
        }
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
    let mut values = Vec::new();
    for entry in AgentRuntime::catalog() {
        values.push(runtime_value(state, *entry).await?);
    }
    Ok(values)
}

async fn runtime_value(
    state: &AppState,
    entry: AgentRuntimeCatalogEntry,
) -> Result<RuntimeResponse, GatewayError> {
    let credential = match load_credential(state, entry.id).await {
        Ok(value) => Some(value),
        Err(GatewayError::InvalidJsonMessage(_)) | Err(GatewayError::MissingDatabase) => None,
        Err(error) => return Err(error),
    };
    Ok(RuntimeResponse {
        id: entry.id.to_owned(),
        name: entry.name.to_owned(),
        default_api_base: entry.default_api_base.to_owned(),
        connected: credential.is_some(),
        api_base: credential.as_ref().map(|value| value.api_base.clone()),
        masked_api_key: credential.map(|value| mask(&value.api_key)),
    })
}

fn credential_name(runtime: &str) -> String {
    format!("agent-runtime:{runtime}")
}

fn canonical_runtime(runtime: &str) -> Result<&'static str, GatewayError> {
    normalize_runtime(runtime)
        .ok_or_else(|| GatewayError::InvalidJsonMessage(format!("unsupported runtime: {runtime}")))
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
