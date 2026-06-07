use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};

use crate::{errors::GatewayError, proxy::state::AppState};

use super::{
    config::{bot_token_key, load_agent, load_secret, slack_config},
    web_api,
};

pub async fn members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let pool = crate::http::managed_agents::db(&state, &headers)?;
    let agent = load_agent(pool, &agent_id).await?;
    let config = slack_config(&agent)?;
    let bot_token = load_secret(&state, &bot_token_key(&agent.id, &config)).await?;
    let users =
        web_api::list_users(&state.http, &state.config.slack.api_base_url, &bot_token).await?;
    Ok(Json(serde_json::json!({ "members": users })))
}

#[derive(serde::Deserialize)]
pub struct UpdateAccessBody {
    pub access: String,
    pub allowed_user_ids: Option<Vec<String>>,
}

pub async fn update_access(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(body): Json<UpdateAccessBody>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    if !["only_me", "selected_users", "everyone"].contains(&body.access.as_str()) {
        return Err(GatewayError::InvalidJsonMessage(
            "access must be only_me, selected_users, or everyone".to_owned(),
        ));
    }
    let pool = crate::http::managed_agents::db(&state, &headers)?;
    load_agent(pool, &agent_id).await?;
    if !crate::db::managed_agents::channels::repository::exists(pool, &agent_id, "slack").await? {
        return Err(GatewayError::NotFound(
            "no Slack channel configured for this agent".to_owned(),
        ));
    }
    let patch = serde_json::json!({
        "access": body.access,
        "allowed_user_ids": body.allowed_user_ids.unwrap_or_default(),
    });
    crate::db::managed_agents::channels::repository::update_config(pool, &agent_id, "slack", patch)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
