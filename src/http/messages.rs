use std::sync::Arc;

use axum::{body::Bytes, extract::State, http::HeaderMap, response::Response};
use serde_json::Value;

use crate::{
    errors::GatewayError,
    http::{api_keys::require_gateway_key, credential_overrides, llm},
    proxy::state::AppState,
};

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_gateway_key(&state, &headers).await?;

    let body: Value = serde_json::from_slice(&body).map_err(GatewayError::InvalidJson)?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .ok_or(GatewayError::MissingModel)?;
    let route = credential_overrides::apply(&state, state.router.resolve(model)?).await?;

    let prepared = route
        .handler
        .transform_request(body, &route.deployment, &headers)?;
    let stream = prepared.stream;

    let upstream =
        llm::send_request(&state.http, route.deployment.messages_url(), prepared).await?;
    let response_headers = route
        .handler
        .transform_response_headers(upstream.headers(), stream);
    Ok(llm::build_response(upstream, response_headers).await)
}
