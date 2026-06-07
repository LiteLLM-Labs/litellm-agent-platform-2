use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Serialize;

use crate::{
    db::managed_agents::mcp_servers::{
        repository,
        schema::{CreateManagedMcpServer, ManagedMcpServerRow, UpdateManagedMcpServer},
    },
    errors::GatewayError,
    proxy::state::AppState,
};

#[derive(Debug, Serialize)]
pub struct McpServersResponse<T> {
    pub mcp_servers: T,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub ok: bool,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<CreateManagedMcpServer>,
) -> Result<(StatusCode, Json<ManagedMcpServerRow>), GatewayError> {
    let pool = super::db(&state, &headers)?;
    let row = repository::create(pool, input).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<McpServersResponse<Vec<ManagedMcpServerRow>>>, GatewayError> {
    let pool = super::db(&state, &headers)?;
    Ok(Json(McpServersResponse {
        mcp_servers: repository::list(pool).await?,
    }))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<ManagedMcpServerRow>, GatewayError> {
    let pool = super::db(&state, &headers)?;
    let row = repository::get(pool, &server_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("not found".to_owned()))?;
    Ok(Json(row))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(input): Json<UpdateManagedMcpServer>,
) -> Result<Json<ManagedMcpServerRow>, GatewayError> {
    let pool = super::db(&state, &headers)?;
    let row = repository::update(pool, &server_id, input)
        .await?
        .ok_or_else(|| GatewayError::NotFound("not found".to_owned()))?;
    Ok(Json(row))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<DeleteResponse>, GatewayError> {
    let pool = super::db(&state, &headers)?;
    Ok(Json(DeleteResponse {
        ok: repository::delete(pool, &server_id).await?,
    }))
}
