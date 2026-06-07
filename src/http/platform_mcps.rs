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
    db::managed_agents::{memory, messages, registry, sessions},
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, state::AppState},
};

pub const PLATFORM_SESSION_MCP_ID: &str = "read_platform_session";
pub const AGENT_MEMORY_MCP_ID: &str = "agent_memory";
pub const PLATFORM_MCP_SERVER_NAME: &str = "platform";

#[derive(Debug, Clone, Serialize)]
pub struct PlatformMcp {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub fn platform_mcps() -> [PlatformMcp; 2] {
    [
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
            let result = call_tool(pool, &agent_id, params).await?;
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
    matches!(id, PLATFORM_SESSION_MCP_ID | AGENT_MEMORY_MCP_ID)
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
    ]
}

async fn call_tool(pool: &PgPool, agent_id: &str, params: Value) -> Result<Value, GatewayError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::InvalidJsonMessage("tool name is required".to_owned()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let payload = match name {
        PLATFORM_SESSION_MCP_ID => read_platform_session(pool, arguments).await?,
        AGENT_MEMORY_MCP_ID => agent_memory(pool, agent_id, arguments).await?,
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

async fn read_platform_session(pool: &PgPool, arguments: Value) -> Result<Value, GatewayError> {
    let session_id = required_str(&arguments, "session_id")?;
    let session = sessions::repository::get(pool, session_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("session not found".to_owned()))?;
    let rows = messages::repository::list(pool, session_id).await?;
    Ok(json!({
        "session": session,
        "messages": rows.into_iter().map(|row| {
            json!({
                "id": row.id,
                "seq": row.seq,
                "info": serde_json::from_str::<Value>(&row.info_json).unwrap_or(Value::String(row.info_json)),
                "parts": serde_json::from_str::<Value>(&row.parts_json).unwrap_or(Value::String(row.parts_json))
            })
        }).collect::<Vec<_>>()
    }))
}

async fn agent_memory(
    pool: &PgPool,
    agent_id: &str,
    arguments: Value,
) -> Result<Value, GatewayError> {
    if registry::repository::get(pool, agent_id).await?.is_none() {
        return Err(GatewayError::UnknownAgent(agent_id.to_owned()));
    }
    match required_str(&arguments, "action")? {
        "list" => Ok(json!({ "memories": memory::repository::list(pool, agent_id).await? })),
        "get" => {
            let key = required_str(&arguments, "key")?;
            let row = memory::repository::list(pool, agent_id)
                .await?
                .into_iter()
                .find(|row| row.key == key);
            Ok(json!({ "memory": row }))
        }
        "set" => {
            let key = required_str(&arguments, "key")?.to_owned();
            let value = required_str(&arguments, "value")?.to_owned();
            let always_on = arguments.get("always_on").and_then(Value::as_bool);
            Ok(
                json!({ "memory": memory::repository::store(pool, agent_id, key, value, always_on).await? }),
            )
        }
        action => Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported memory action: {action}"
        ))),
    }
}

fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, GatewayError> {
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
