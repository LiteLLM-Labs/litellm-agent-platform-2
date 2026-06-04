use std::sync::Arc;

use axum::{extract::State, http::HeaderMap, Json};
use serde_json::{json, Value};

use crate::{
    errors::GatewayError,
    http::{agents::configured_agent_values, api_keys::require_gateway_key},
    proxy::state::AppState,
};

pub async fn capabilities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    require_gateway_key(&state, &headers).await?;
    Ok(Json(json!({
        "providers": providers(&state),
        "models": model_entries(&state),
        "endpoints": [
            "/health",
            "/openapi.json",
            "/v1/models",
            "/v1/messages",
            "/v1/responses",
            "/mcp",
            "/mcp/{server_id}",
            "/api/agents",
            "/api/agents/{agent_id}/run"
        ],
        "mcp_servers": state.config.mcp_servers.keys().cloned().collect::<Vec<_>>(),
        "agents": configured_agent_values(&state)
    })))
}

pub async fn models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    require_gateway_key(&state, &headers).await?;
    Ok(Json(json!({
        "object": "list",
        "data": model_entries(&state)
    })))
}

fn model_entries(state: &AppState) -> Vec<Value> {
    state
        .config
        .model_list
        .iter()
        .map(|model| {
            json!({
                "id": model.model_name,
                "object": "model",
                "provider": provider_from_model(&model.litellm_params.model)
            })
        })
        .collect()
}

fn providers(state: &AppState) -> Vec<String> {
    let mut providers = state
        .config
        .model_list
        .iter()
        .map(|model| provider_from_model(&model.litellm_params.model))
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();
    providers
}

fn provider_from_model(model: &str) -> String {
    model
        .split_once('/')
        .map(|(provider, _)| provider.to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}
