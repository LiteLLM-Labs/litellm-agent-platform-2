use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    db::{credentials, managed_agents::harnesses},
    errors::GatewayError,
    http::{
        agent_runtime_tools::{runtime_tools, RuntimeTool},
        agent_runtimes::load_credential,
        runtime_resolution::harness_credential_name,
    },
    proxy::{
        auth::master_key::require_master_key,
        credential_crypto,
        provider_credentials::mask_api_key,
        state::AppState,
    },
    sdk::agents::{AgentRuntime, CLAUDE_MANAGED_AGENTS, CURSOR, OPENCODE},
};

/// IDs that are reserved for static (built-in) runtimes and cannot be used as
/// custom harness aliases.
const RESERVED_ALIASES: &[&str] = &[CLAUDE_MANAGED_AGENTS, CURSOR, OPENCODE, "claude_agents"];

#[derive(Debug, Serialize)]
pub struct HarnessResponse {
    pub alias: String,
    pub api_spec: String,
    pub display_name: String,
    pub api_base: String,
    pub is_default: bool,
    pub connected: bool,
    pub masked_api_key: Option<String>,
    pub tools: Vec<RuntimeTool>,
}

#[derive(Debug, Serialize)]
pub struct HarnessesResponse {
    pub harnesses: Vec<HarnessResponse>,
}

#[derive(Debug, Deserialize)]
pub struct CreateHarnessRequest {
    pub alias: String,
    pub api_spec: String,
    pub api_base: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHarnessRequest {
    pub api_key: Option<String>,
    pub api_base: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeleteHarnessResponse {
    pub ok: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HarnessesResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let harnesses = build_harnesses_list(&state, pool).await?;
    Ok(Json(HarnessesResponse { harnesses }))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<CreateHarnessRequest>,
) -> Result<Json<HarnessesResponse>, GatewayError> {
    require_admin(&state, &headers)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;

    // Validate alias
    validate_alias(&input.alias)?;

    // Validate api_spec is a known runtime id
    let valid_spec = {
        let registry = crate::sdk::providers::runtime_registry();
        registry.validate_id(&input.api_spec)
    };
    if !valid_spec {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "unknown api_spec: {}",
            input.api_spec
        )));
    }

    // Check alias does not already exist
    if harnesses::repository::get_by_alias(pool, &input.alias)
        .await?
        .is_some()
    {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "harness alias already exists: {}",
            input.alias
        )));
    }

    let api_key = input.api_key.trim().to_owned();
    if api_key.is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "api_key is required".to_owned(),
        ));
    }
    let api_base = input.api_base.trim().to_owned();

    // Insert harness row first; if credential upsert fails we roll back by deleting the row.
    let cred_name = harness_credential_name(&input.alias);
    harnesses::repository::create(pool, &input.alias, &input.api_spec, &api_base).await?;

    let key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;
    let values = json!({
        "api_key": credential_crypto::encrypt_value(&api_key, &key)?,
        "api_base": credential_crypto::encrypt_value(&api_base, &key)?,
    });
    if let Err(err) = credentials::upsert(pool, &cred_name, values, json!({}), "ui").await {
        // Compensate: remove the orphan row
        let _ = harnesses::repository::delete(pool, &input.alias).await;
        return Err(err);
    }

    Ok(Json(HarnessesResponse {
        harnesses: build_harnesses_list(&state, pool).await?,
    }))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(alias): Path<String>,
    Json(input): Json<UpdateHarnessRequest>,
) -> Result<Json<HarnessesResponse>, GatewayError> {
    require_admin(&state, &headers)?;

    // Reject attempts to update built-in runtimes via this endpoint
    if RESERVED_ALIASES.contains(&alias.as_str()) {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "cannot update built-in runtime via this endpoint: {alias}"
        )));
    }

    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;

    let row = harnesses::repository::get_by_alias(pool, &alias)
        .await?
        .ok_or_else(|| GatewayError::NotFound(format!("harness not found: {alias}")))?;

    let enc_key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;

    // Load the existing credential so we can merge
    let cred_name = harness_credential_name(&alias);
    let existing = credentials::get_by_name(pool, &cred_name).await?;

    let (current_api_key, current_api_base) = if let Some(ref cred_row) = existing {
        let vals = cred_row.credential_values.as_object().ok_or_else(|| {
            GatewayError::InvalidConfig("harness credential_values must be an object".to_owned())
        })?;
        let k = decrypt_field(vals, "api_key", &enc_key)?;
        let b = decrypt_field(vals, "api_base", &enc_key)?;
        (k, b)
    } else {
        (String::new(), row.api_base.clone())
    };

    let new_api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&current_api_key)
        .to_owned();

    let new_api_base = input
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&current_api_base)
        .to_owned();

    // Re-encrypt and upsert credential
    let values = json!({
        "api_key": credential_crypto::encrypt_value(&new_api_key, &enc_key)?,
        "api_base": credential_crypto::encrypt_value(&new_api_base, &enc_key)?,
    });
    credentials::upsert(pool, &cred_name, values, json!({}), "ui").await?;

    // Update api_base in harness row if it changed
    if new_api_base != row.api_base {
        harnesses::repository::update_api_base(pool, &alias, &new_api_base).await?;
    }

    Ok(Json(HarnessesResponse {
        harnesses: build_harnesses_list(&state, pool).await?,
    }))
}

pub async fn delete_harness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(alias): Path<String>,
) -> Result<(StatusCode, Json<DeleteHarnessResponse>), GatewayError> {
    require_admin(&state, &headers)?;

    // Reject attempts to delete built-in runtimes
    if RESERVED_ALIASES.contains(&alias.as_str()) {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "cannot delete built-in runtime: {alias}"
        )));
    }

    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;

    // Delete row first; if credential delete fails the harness is gone and won't be listed.
    // Credential orphan is harmless (no alias to resolve it). Reverse order risks a
    // listed harness with no credential — sessions would fail with a confusing error.
    harnesses::repository::delete(pool, &alias).await?;

    let cred_name = harness_credential_name(&alias);
    let _ = credentials::delete_by_name(pool, &cred_name).await;

    Ok((StatusCode::OK, Json(DeleteHarnessResponse { ok: true })))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), GatewayError> {
    require_master_key(headers, state.config.general_settings.master_key.as_deref())
}

fn validate_alias(alias: &str) -> Result<(), GatewayError> {
    if alias.is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "alias must not be empty".to_owned(),
        ));
    }
    if !alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(GatewayError::InvalidJsonMessage(
            "alias may only contain [a-zA-Z0-9_-]".to_owned(),
        ));
    }
    if RESERVED_ALIASES.contains(&alias) {
        return Err(GatewayError::InvalidJsonMessage(format!(
            "alias is reserved: {alias}"
        )));
    }
    Ok(())
}

async fn build_harnesses_list(
    state: &AppState,
    pool: &sqlx::PgPool,
) -> Result<Vec<HarnessResponse>, GatewayError> {
    let mut result = Vec::new();

    // Built-in / default runtimes
    for entry in AgentRuntime::catalog() {
        let credential = match load_credential(state, entry.id).await {
            Ok(c) => Some(c),
            Err(GatewayError::InvalidJsonMessage(_)) | Err(GatewayError::MissingDatabase) => None,
            Err(e) => return Err(e),
        };
        result.push(HarnessResponse {
            alias: entry.id.to_owned(),
            api_spec: entry.id.to_owned(),
            display_name: entry.name.to_owned(),
            api_base: credential
                .as_ref()
                .map(|c| c.api_base.clone())
                .unwrap_or_else(|| entry.default_api_base.to_owned()),
            is_default: true,
            connected: credential.is_some(),
            masked_api_key: credential.map(|c| mask_api_key(&c.api_key)),
            tools: runtime_tools(entry.id).to_vec(),
        });
    }

    // Custom harnesses from DB
    let custom = harnesses::repository::list(pool).await?;
    let enc_key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())
            .ok();

    for harness in custom {
        let (connected, masked_api_key, resolved_api_base) =
            if let Some(ref key) = enc_key {
                match load_harness_api_key(pool, &harness.alias, key).await {
                    Ok((api_key, api_base)) => {
                        let masked = mask_api_key(&api_key);
                        (true, Some(masked), api_base)
                    }
                    Err(_) => (false, None, harness.api_base.clone()),
                }
            } else {
                (false, None, harness.api_base.clone())
            };

        result.push(HarnessResponse {
            alias: harness.alias.clone(),
            api_spec: harness.api_spec.clone(),
            display_name: harness.alias.clone(),
            api_base: resolved_api_base,
            is_default: false,
            connected,
            masked_api_key,
            tools: runtime_tools(&harness.api_spec).to_vec(),
        });
    }

    Ok(result)
}

async fn load_harness_api_key(
    pool: &sqlx::PgPool,
    alias: &str,
    enc_key: &str,
) -> Result<(String, String), GatewayError> {
    let cred_name = harness_credential_name(alias);
    let row = credentials::get_by_name(pool, &cred_name)
        .await?
        .ok_or_else(|| {
            GatewayError::InvalidJsonMessage(format!("no credential for harness: {alias}"))
        })?;
    let vals = row.credential_values.as_object().ok_or_else(|| {
        GatewayError::InvalidConfig("harness credential_values must be an object".to_owned())
    })?;
    let api_key = decrypt_field(vals, "api_key", enc_key)?;
    let api_base = decrypt_field(vals, "api_base", enc_key)?;
    Ok((api_key, api_base))
}

fn decrypt_field(
    values: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    key: &str,
) -> Result<String, GatewayError> {
    let enc = values
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            GatewayError::InvalidConfig(format!("harness credential missing field: {field}"))
        })?;
    credential_crypto::decrypt_value(enc, key)
}
