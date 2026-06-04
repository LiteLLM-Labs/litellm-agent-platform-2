use std::sync::Arc;

use axum::{extract::State, http::HeaderMap, Json};
use serde::Deserialize;

use crate::{
    db::managed_agents::settings::{repository, schema::ObservabilitySettings},
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, state::AppState},
};

#[derive(Debug, Deserialize)]
pub struct UpdateObservabilitySettings {
    store_spend_logs: Option<bool>,
    store_prompts_in_spend_logs: Option<bool>,
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ObservabilitySettings>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    Ok(Json(
        repository::get_observability(pool, &state.observability_settings_defaults).await?,
    ))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<UpdateObservabilitySettings>,
) -> Result<Json<ObservabilitySettings>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let current =
        repository::get_observability(pool, &state.observability_settings_defaults).await?;
    let next = ObservabilitySettings {
        store_spend_logs: input.store_spend_logs.unwrap_or(current.store_spend_logs),
        store_prompts_in_spend_logs: input
            .store_prompts_in_spend_logs
            .unwrap_or(current.store_prompts_in_spend_logs),
    };
    Ok(Json(repository::save_observability(pool, &next).await?))
}
