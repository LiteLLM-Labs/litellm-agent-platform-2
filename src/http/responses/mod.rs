use std::sync::Arc;

use axum::{body::Bytes, extract::State, http::HeaderMap, response::Response};
use serde_json::Value;

use crate::{
    errors::GatewayError,
    http::{
        api_keys::require_gateway_key,
        credential_overrides, llm,
        responses::{
            output::{create_response_body, json_response, parse_json},
            request::ResponsesRequest,
            stream::{response_stream_body, sse_response},
        },
    },
    proxy::state::AppState,
};

mod output;
mod request;
mod sse;
mod stream;
mod util;

pub async fn responses(
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
    let request = ResponsesRequest::from_value(body)?;
    let messages_body = request.to_messages_body()?;

    let prepared = route
        .handler
        .transform_request(messages_body, &route.deployment, &headers)?;
    let stream = prepared.stream;
    let upstream =
        llm::send_request(&state.http, route.deployment.messages_url(), prepared).await?;

    if !upstream.status().is_success() {
        let response_headers = route
            .handler
            .transform_response_headers(upstream.headers(), stream);
        return Ok(llm::build_response(upstream, response_headers).await);
    }

    let status = upstream.status();
    let body = upstream.bytes().await.map_err(GatewayError::Upstream)?;
    if stream {
        Ok(sse_response(status, response_stream_body(&request, &body)?))
    } else {
        Ok(json_response(
            status,
            create_response_body(&request, parse_json(&body)?)?,
        ))
    }
}
