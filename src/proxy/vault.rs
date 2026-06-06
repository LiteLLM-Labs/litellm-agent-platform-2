use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::credentials,
    errors::GatewayError,
    proxy::{config::GatewayConfig, credential_crypto},
};

#[derive(Debug, Clone, Serialize)]
pub struct VaultKeyEntry {
    pub key: String,
    pub source: String,
}

pub async fn save(
    pool: &PgPool,
    config: &GatewayConfig,
    user_id: &str,
    key: &str,
    value: &str,
) -> Result<(), GatewayError> {
    validate_key(key)?;
    let encryption_key =
        credential_crypto::encryption_key(config.general_settings.master_key.as_deref())?;
    credentials::upsert(
        pool,
        &credential_name(user_id, key),
        json!({ "value": credential_crypto::encrypt_value(value, &encryption_key)? }),
        json!({ "source": "vault", "user_id": user_id, "key": key }),
        user_id,
    )
    .await
}

pub async fn load(
    pool: &PgPool,
    config: &GatewayConfig,
    user_id: &str,
    key: &str,
) -> Result<Option<String>, GatewayError> {
    validate_key(key)?;
    let Some(row) = credentials::get_by_name(pool, &credential_name(user_id, key)).await? else {
        return Ok(None);
    };
    let encryption_key =
        credential_crypto::encryption_key(config.general_settings.master_key.as_deref())?;
    let values = row.credential_values.as_object().ok_or_else(|| {
        GatewayError::InvalidConfig("vault credential_values must be an object".to_owned())
    })?;
    let encrypted = values.get("value").and_then(Value::as_str).ok_or_else(|| {
        GatewayError::InvalidConfig("vault credential is missing value".to_owned())
    })?;
    credential_crypto::decrypt_value(encrypted, &encryption_key).map(Some)
}

pub async fn list(pool: &PgPool, user_id: &str) -> Result<Vec<VaultKeyEntry>, GatewayError> {
    let prefix = credential_prefix(user_id);
    credentials::list_by_prefix(pool, &prefix)
        .await?
        .into_iter()
        .map(|row| {
            let key = row
                .credential_info
                .as_ref()
                .and_then(|info| info.get("key"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| row.credential_name.trim_start_matches(&prefix).to_owned());
            Ok(VaultKeyEntry {
                key,
                source: "vault".to_owned(),
            })
        })
        .collect()
}

pub async fn delete(pool: &PgPool, user_id: &str, key: &str) -> Result<bool, GatewayError> {
    validate_key(key)?;
    credentials::delete_by_name(pool, &credential_name(user_id, key)).await
}

pub fn credential_name(user_id: &str, key: &str) -> String {
    format!("{}{key}", credential_prefix(user_id))
}

fn credential_prefix(user_id: &str) -> String {
    format!("vault:{user_id}:")
}

fn validate_key(key: &str) -> Result<(), GatewayError> {
    if key.trim().is_empty() || key.contains('/') || key.contains(':') {
        return Err(GatewayError::InvalidJsonMessage(
            "vault key must be non-empty and cannot contain / or :".to_owned(),
        ));
    }
    Ok(())
}
