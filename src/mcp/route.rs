use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method},
    response::Response,
};

use crate::{
    db::managed_agents::mcp_servers::repository,
    errors::GatewayError,
    mcp::registry::{McpServer, McpServerRegistry},
    proxy::{auth::master_key::require_any_gateway_key, state::AppState},
};

const SERVER_HEADER: &str = "x-litellm-mcp-server";

pub async fn streamable_http(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let server_id = select_server_id(&state.mcp_servers, &headers, query.get("server"))?;
    let server = resolve_server(&state, server_id).await?;
    crate::mcp::upstream::forward_streamable_http(
        &state.http,
        server.as_ref(),
        method,
        &headers,
        body,
    )
    .await
}

pub async fn streamable_http_server(
    State(state): State<Arc<AppState>>,
    Path(server_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let server = resolve_server(&state, &server_id).await?;
    crate::mcp::upstream::forward_streamable_http(
        &state.http,
        server.as_ref(),
        method,
        &headers,
        body,
    )
    .await
}

async fn resolve_server<'a>(
    state: &'a AppState,
    server_id: &str,
) -> Result<std::borrow::Cow<'a, McpServer>, GatewayError> {
    match state.mcp_servers.resolve(server_id) {
        Ok(server) => return Ok(std::borrow::Cow::Borrowed(server)),
        Err(GatewayError::UnknownMcpServer(_)) => {}
        Err(error) => return Err(error),
    }

    let Some(pool) = state.db.as_ref() else {
        return Err(GatewayError::UnknownMcpServer(server_id.to_owned()));
    };
    let row = repository::get(pool, server_id)
        .await?
        .ok_or_else(|| GatewayError::UnknownMcpServer(server_id.to_owned()))?;
    Ok(std::borrow::Cow::Owned(McpServer::from_managed_row(&row)?))
}

fn select_server_id<'a>(
    registry: &'a McpServerRegistry,
    headers: &'a HeaderMap,
    query_server: Option<&'a String>,
) -> Result<&'a str, GatewayError> {
    if let Some(server_id) = query_server {
        return Ok(server_id.as_str());
    }

    if let Some(server_id) = headers
        .get(SERVER_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(server_id);
    }

    registry
        .only_server_id()
        .ok_or(GatewayError::MissingMcpServer)
}
