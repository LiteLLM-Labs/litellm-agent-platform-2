use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, state::AppState},
};

mod slack;
mod tools;

pub const PLATFORM_SESSION_MCP_ID: &str = "read_platform_session";
pub const AGENT_MEMORY_MCP_ID: &str = "agent_memory";
pub const SEND_SLACK_MESSAGE_MCP_ID: &str = "send_slack_message";
pub const PLATFORM_MCP_SERVER_NAME: &str = "platform";

#[derive(Debug, Clone, Serialize)]
pub struct PlatformMcp {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub fn platform_mcps() -> Vec<PlatformMcp> {
    vec![
        PlatformMcp {
            id: PLATFORM_SESSION_MCP_ID,
            name: "Read platform session",
            description: "Read persisted platform session messages for debugging and handoff.",
        },
        PlatformMcp {
            id: AGENT_MEMORY_MCP_ID,
            name: "Read/Write agent memory",
            description: "List, read, and update DB-backed memory for a platform agent.",
        },
        PlatformMcp {
            id: SEND_SLACK_MESSAGE_MCP_ID,
            name: "Send Slack message",
            description: "Send a channel message or DM from this agent's connected Slack bot.",
        },
    ]
}

pub fn selected_platform_mcp_ids(config: &Value) -> Vec<String> {
    config
        .get("platform_mcp_ids")
        .or_else(|| config.get("platformMcpIds"))
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|id| is_platform_mcp(id))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn platform_mcp_servers(
    state: &AppState,
    agent_id: &str,
    config: &Value,
) -> Result<Vec<Value>, GatewayError> {
    let ids = selected_platform_mcp_ids(config);
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![json!({
        "name": PLATFORM_MCP_SERVER_NAME,
        "type": "url",
        "url": platform_mcp_url(state, agent_id)?
    })])
}

pub fn platform_mcp_toolsets(config: &Value) -> Vec<Value> {
    let ids = selected_platform_mcp_ids(config);
    if ids.is_empty() {
        return Vec::new();
    }
    vec![json!({
        "type": "mcp_toolset",
        "mcp_server_name": PLATFORM_MCP_SERVER_NAME,
        "default_config": {
            "enabled": false,
            "permission_policy": { "type": "always_allow" }
        },
        "configs": ids.into_iter().map(|id| json!({ "name": id, "enabled": true })).collect::<Vec<_>>()
    })]
}

pub fn platform_mcp_url(state: &AppState, agent_id: &str) -> Result<String, GatewayError> {
    let Some(base_url) = state
        .config
        .general_settings
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(GatewayError::InvalidConfig(
            "general_settings.public_base_url is required for platform MCPs".to_owned(),
        ));
    };
    Ok(format!(
        "{}/mcp/platform/{}",
        base_url.trim_end_matches('/'),
        agent_id
    ))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    Ok(Json(json!({ "platform_mcps": platform_mcps() })))
}

pub async fn serve(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<Value>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let response = match request.method.as_str() {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "litellm-platform", "version": env!("CARGO_PKG_VERSION") }
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "result": { "tools": tool_defs() }
        }),
        "tools/call" => {
            let Some(params) = request.params else {
                return Ok(Json(rpc_error(request.id, -32602, "params are required")));
            };
            let result = call_tool(&state, pool, &agent_id, params).await?;
            json!({ "jsonrpc": "2.0", "id": request.id, "result": result })
        }
        "notifications/initialized" => json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "result": {}
        }),
        _ => rpc_error(request.id, -32601, "method not found"),
    };
    Ok(Json(response))
}

fn is_platform_mcp(id: &str) -> bool {
    matches!(
        id,
        PLATFORM_SESSION_MCP_ID | AGENT_MEMORY_MCP_ID | SEND_SLACK_MESSAGE_MCP_ID
    )
}

fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": PLATFORM_SESSION_MCP_ID,
            "description": "Read persisted platform session messages by session_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": AGENT_MEMORY_MCP_ID,
            "description": "List, read, or update DB-backed memory for this platform agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "get", "set"] },
                    "key": { "type": "string" },
                    "value": { "type": "string" },
                    "always_on": { "type": "boolean" }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": SEND_SLACK_MESSAGE_MCP_ID,
            "description": "Send a Slack channel message or DM using this agent's connected Slack bot.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "channel_id": {
                        "type": "string",
                        "description": "Slack channel ID, such as C123. When omitted, sends a DM."
                    },
                    "user_id": {
                        "type": "string",
                        "description": "Slack user ID, such as U123. Used for DMs only."
                    },
                    "email": {
                        "type": "string",
                        "description": "Slack user email. Used for DMs when user_id is omitted."
                    },
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }
        }),
    ]
}

async fn call_tool(
    state: &AppState,
    pool: &PgPool,
    agent_id: &str,
    params: Value,
) -> Result<Value, GatewayError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::InvalidJsonMessage("tool name is required".to_owned()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let payload = match name {
        PLATFORM_SESSION_MCP_ID => tools::read_platform_session(pool, arguments).await?,
        AGENT_MEMORY_MCP_ID => tools::agent_memory(pool, agent_id, arguments).await?,
        SEND_SLACK_MESSAGE_MCP_ID => slack::send_message(state, pool, agent_id, arguments).await?,
        _ => {
            return Ok(json!({
                "isError": true,
                "content": [{ "type": "text", "text": format!("unknown tool: {name}") }]
            }))
        }
    };
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }]
    }))
}

pub(crate) fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, GatewayError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GatewayError::InvalidJsonMessage(format!("{field} is required")))
}

fn rpc_error(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}
