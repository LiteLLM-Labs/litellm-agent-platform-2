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

mod definitions;
mod factory;
mod factory_slack;
pub(crate) mod factory_slack_app;
mod factory_slack_manifest;
mod selection;
mod session_management;
mod slack;
mod tools;

pub const PLATFORM_SESSION_MCP_ID: &str = "read_platform_session";
pub const SEND_PLATFORM_SESSION_MESSAGE_MCP_ID: &str = "send_platform_session_message";
pub const AGENT_MEMORY_MCP_ID: &str = "agent_memory";
pub const SEND_SLACK_MESSAGE_MCP_ID: &str = "send_slack_message";
pub const PLATFORM_MCP_SERVER_NAME: &str = "platform";
pub const CREATE_MANAGED_AGENT_MCP_ID: &str = "create_managed_agent";
pub const CONNECT_AGENT_TO_SLACK_MCP_ID: &str = "connect_agent_to_slack";
pub const LIST_SLACK_AGENT_BINDINGS_MCP_ID: &str = "list_slack_agent_bindings";
pub const RUN_SUB_AGENT_MCP_ID: &str = "run_sub_agent";

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
            id: SEND_PLATFORM_SESSION_MESSAGE_MCP_ID,
            name: "Send platform session message",
            description: "Send a user message into a platform session and resume that agent run.",
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
        PlatformMcp {
            id: CREATE_MANAGED_AGENT_MCP_ID,
            name: "Create managed agent",
            description: "Create a Claude managed agent from a Slack or platform request.",
        },
        PlatformMcp {
            id: CONNECT_AGENT_TO_SLACK_MCP_ID,
            name: "Connect agent to Slack",
            description:
                "Create a dedicated Slack app for a managed agent and return its install URL.",
        },
        PlatformMcp {
            id: LIST_SLACK_AGENT_BINDINGS_MCP_ID,
            name: "List Slack agent bindings",
            description: "List channel bindings created by this platform agent factory.",
        },
        PlatformMcp {
            id: RUN_SUB_AGENT_MCP_ID,
            name: "Run sub-agent",
            description:
                "Run one of this agent's explicitly attached LAP sub-agents and return its session.",
        },
    ]
}

pub use selection::selected_platform_mcp_ids;
pub(crate) use selection::sub_agent_ids;

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
        "initialize" => initialize_response(request.id),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "result": { "tools": definitions::tool_defs() }
        }),
        "tools/call" => {
            let Some(params) = request.params else {
                return Ok(Json(rpc_error(request.id, -32602, "params are required")));
            };
            let result = call_tool(state.clone(), pool, &agent_id, params).await?;
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

async fn call_tool(
    state: Arc<AppState>,
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
        PLATFORM_SESSION_MCP_ID => {
            session_management::read_platform_session(pool, arguments).await?
        }
        SEND_PLATFORM_SESSION_MESSAGE_MCP_ID => {
            session_management::send_platform_session_message(
                state.clone(),
                pool.clone(),
                arguments,
            )
            .await?
        }
        AGENT_MEMORY_MCP_ID => tools::agent_memory(pool, agent_id, arguments).await?,
        SEND_SLACK_MESSAGE_MCP_ID => {
            slack::send_message(state.as_ref(), pool, agent_id, arguments).await?
        }
        CREATE_MANAGED_AGENT_MCP_ID => {
            factory::create_managed_agent(state.as_ref(), pool, arguments).await?
        }
        CONNECT_AGENT_TO_SLACK_MCP_ID => {
            factory_slack::connect_agent_to_slack(state.as_ref(), pool, agent_id, arguments).await?
        }
        LIST_SLACK_AGENT_BINDINGS_MCP_ID => {
            factory_slack::list_slack_bindings(pool, agent_id).await?
        }
        RUN_SUB_AGENT_MCP_ID => {
            tools::run_sub_agent(state.clone(), pool.clone(), agent_id, arguments).await?
        }
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

pub(super) fn public_base_url(state: &AppState) -> Result<String, GatewayError> {
    state
        .config
        .general_settings
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            GatewayError::InvalidConfig(
                "general_settings.public_base_url is required for platform MCPs".to_owned(),
            )
        })
}

fn rpc_error(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn initialize_response(id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "litellm-platform", "version": env!("CARGO_PKG_VERSION") }
        }
    })
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}
