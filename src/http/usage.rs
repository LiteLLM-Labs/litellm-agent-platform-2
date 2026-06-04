use std::sync::Arc;

use axum::{extract::State, http::HeaderMap, Json};
use serde::Serialize;
use sqlx::FromRow;

use crate::{
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, state::AppState},
};

#[derive(Debug, FromRow, Serialize)]
pub struct AgentUsageSummary {
    pub agent_key: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub agents: Vec<AgentUsageSummary>,
}

pub async fn agents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UsageResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let agents = sqlx::query_as::<_, AgentUsageSummary>(
        r#"
        SELECT
          COALESCE(NULLIF(metadata->>'agent_key', ''), 'unknown') AS agent_key,
          COUNT(*)::BIGINT AS request_count,
          COALESCE(SUM(prompt_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(completion_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(spend), 0)::DOUBLE PRECISION AS cost_usd,
          FLOOR(EXTRACT(EPOCH FROM MAX("startTime")) * 1000)::BIGINT AS last_used_at
        FROM "LiteLLM_SpendLogs"
        GROUP BY COALESCE(NULLIF(metadata->>'agent_key', ''), 'unknown')
        ORDER BY cost_usd DESC, request_count DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(GatewayError::Database)?;

    Ok(Json(UsageResponse { agents }))
}
