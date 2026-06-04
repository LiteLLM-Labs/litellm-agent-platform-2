use serde_json::Value;
use sqlx::PgPool;

use crate::errors::GatewayError;

use super::schema::ObservabilitySettings;

const OBSERVABILITY_KEY: &str = "observability";

pub async fn get_observability(
    pool: &PgPool,
    defaults: &ObservabilitySettings,
) -> Result<ObservabilitySettings, GatewayError> {
    let value = sqlx::query_scalar::<_, Value>(
        r#"
        SELECT value
        FROM "LiteLLM_GatewaySettings"
        WHERE key = $1
        "#,
    )
    .bind(OBSERVABILITY_KEY)
    .fetch_optional(pool)
    .await
    .map_err(GatewayError::Database)?;

    let Some(value) = value else {
        return Ok(defaults.clone());
    };
    serde_json::from_value(value).map_err(GatewayError::InvalidJson)
}

pub async fn save_observability(
    pool: &PgPool,
    settings: &ObservabilitySettings,
) -> Result<ObservabilitySettings, GatewayError> {
    let value = serde_json::to_value(settings).map_err(GatewayError::InvalidJson)?;
    sqlx::query(
        r#"
        INSERT INTO "LiteLLM_GatewaySettings" (key, value, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (key) DO UPDATE SET
          value = EXCLUDED.value,
          updated_at = NOW()
        "#,
    )
    .bind(OBSERVABILITY_KEY)
    .bind(value)
    .execute(pool)
    .await
    .map_err(GatewayError::Database)?;

    Ok(settings.clone())
}
