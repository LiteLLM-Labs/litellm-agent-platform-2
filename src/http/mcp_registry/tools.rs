use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    db::{
        credentials,
        mcp_servers::{repository, schema::McpServerRow},
    },
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, credential_crypto, state::AppState},
};

use super::substitute_vars;

// ── response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ToolsResponse {
    pub server_id: String,
    pub tools: Vec<Value>,
}

#[derive(Deserialize)]
pub struct TestToolsRequest {
    pub variables: HashMap<String, String>,
}

// ── shared helpers ─────────────────────────────────────────────────────────────

/// Build a variable substitution map from a server's `mcp_info["variables"]` array.
///
/// - `scope = "instance"`: decrypt from `server.credentials[name]`, fall back to plaintext.
/// - `scope = "per_user"`: fetch from vault as `mcp_var:{server_id}:{var_name}` owned by user_id.
pub async fn build_vars_map(
    pool: &sqlx::PgPool,
    server: &McpServerRow,
    user_id: &str,
    enc_key: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let vars = match server.mcp_info.get("variables").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => return map,
    };

    for var in &vars {
        let name = match var.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let scope = var
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("instance");

        let value: Option<String> = if scope == "per_user" {
            let vault_key = format!("mcp_var:{}:{}", server.server_id, name);
            credentials::get_personal_by_name(pool, &vault_key, user_id)
                .await
                .ok()
                .flatten()
                .and_then(|row| {
                    row.credential_values
                        .get("value")
                        .and_then(|v| v.as_str())
                        .and_then(|enc| credential_crypto::decrypt_value(enc, enc_key).ok())
                })
        } else {
            server
                .credentials
                .get(name)
                .and_then(|v| v.as_str())
                .map(|raw| {
                    credential_crypto::decrypt_value(raw, enc_key)
                        .unwrap_or_else(|_| raw.to_owned())
                })
        };

        if let Some(v) = value {
            map.insert(name.to_owned(), v);
        }
    }

    map
}

pub fn extract_tools_from_response(text: &str, content_type: &str) -> Vec<Value> {
    // SSE (text/event-stream): parse data: lines
    if content_type.contains("event-stream") || text.starts_with("data:") {
        for line in text.lines() {
            let data = line.strip_prefix("data:").map(str::trim).unwrap_or("");
            if data.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                let tools = v
                    .pointer("/result/tools")
                    .or_else(|| v.get("tools"))
                    .and_then(Value::as_array)
                    .cloned();
                if let Some(t) = tools {
                    return t;
                }
            }
        }
        return vec![];
    }
    // JSON: parse directly
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return v
            .pointer("/result/tools")
            .or_else(|| v.get("tools"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
    }
    vec![]
}

/// Inject static headers with variable substitution into a request builder.
fn apply_static_headers(
    mut req: reqwest::RequestBuilder,
    static_headers: &Value,
    vars: &HashMap<String, String>,
) -> reqwest::RequestBuilder {
    if let Some(obj) = static_headers.as_object() {
        for (name, val) in obj {
            if let Some(template) = val.as_str() {
                let resolved = substitute_vars(template, vars);
                if let (Ok(n), Ok(hv)) = (
                    axum::http::HeaderName::from_bytes(name.as_bytes()),
                    axum::http::HeaderValue::from_str(&resolved),
                ) {
                    req = req.header(n, hv);
                }
            }
        }
    }
    req
}

/// Send a `tools/list` JSON-RPC request and extract the tools array.
async fn fetch_tools(
    req: reqwest::RequestBuilder,
) -> Result<Vec<Value>, GatewayError> {
    let res = req
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send()
        .await
        .map_err(GatewayError::Upstream)?;

    if res.status().is_success() {
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let text = res.text().await.map_err(GatewayError::Upstream)?;
        Ok(extract_tools_from_response(&text, &content_type))
    } else {
        Ok(vec![])
    }
}

// ── handlers ───────────────────────────────────────────────────────────────────

/// GET /v1/mcp/server/{server_id}/tools — auth: any configured gateway key.
pub async fn list_tools(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<ToolsResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let server = repository::get(pool, &server_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound(format!("MCP server not found: {server_id}")))?;
    if server.approval_status.as_deref() != Some("active") {
        return Err(GatewayError::NotFound(format!(
            "MCP server not found: {server_id}"
        )));
    }

    let url = server
        .url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidConfig("MCP server has no URL configured".to_owned())
        })?;

    let user_id = super::caller_user_id(&headers, &state);
    let enc_key_opt =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref()).ok();

    let vars: HashMap<String, String> = if let Some(key) = enc_key_opt.as_deref() {
        build_vars_map(pool, &server, &user_id, key).await
    } else {
        HashMap::new()
    };

    let tools_url = substitute_vars(url.trim_end_matches('/'), &vars);
    let mut req = state
        .http
        .post(&tools_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");

    let has_static_headers = server
        .static_headers
        .as_object()
        .is_some_and(|o| !o.is_empty());
    req = apply_static_headers(req, &server.static_headers, &vars);

    if !has_static_headers {
        if let Some(key) = enc_key_opt.as_deref() {
            let cred_name = format!("mcp_user:{}:{}", server_id, user_id);
            let credential: Option<String> =
                credentials::get_personal_by_name(pool, &cred_name, &user_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|r| {
                        r.credential_values
                            .get("value")
                            .and_then(|v| v.as_str())
                            .and_then(|enc| credential_crypto::decrypt_value(enc, key).ok())
                    })
                    .or_else(|| {
                        server
                            .credentials
                            .get("value")
                            .and_then(|v| v.as_str())
                            .and_then(|enc| credential_crypto::decrypt_value(enc, key).ok())
                            .or_else(|| {
                                server
                                    .credentials
                                    .get("api_key")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_owned)
                            })
                    });
            if let Some(cred) = credential {
                req = match server.auth_type.as_deref().unwrap_or("bearer_token") {
                    "api_key" => req.header("x-api-key", cred),
                    "basic" => req.header("Authorization", format!("Basic {cred}")),
                    _ => req.header("Authorization", format!("Bearer {cred}")),
                };
            }
        }
    }

    let tools = fetch_tools(req).await?;
    Ok(Json(ToolsResponse { server_id, tools }))
}

/// POST /v1/mcp/server/{server_id}/tools — test with caller-supplied variable values.
/// Body: `{"variables": {"VAR_NAME": "value", ...}}`
/// Overrides vault lookup with the provided test values. For admin "Run test" flow.
pub async fn test_tools(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(body): Json<TestToolsRequest>,
) -> Result<Json<ToolsResponse>, GatewayError> {
    require_any_gateway_key(&headers, &state)?;
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let server = repository::get(pool, &server_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound(format!("MCP server not found: {server_id}")))?;

    let url = server
        .url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidConfig("MCP server has no URL configured".to_owned())
        })?;

    let enc_key_opt =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref()).ok();

    let mut vars = build_instance_vars(&server, enc_key_opt.as_deref());
    vars.extend(body.variables);

    let tools_url = substitute_vars(url.trim_end_matches('/'), &vars);
    let mut req = state
        .http
        .post(&tools_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");

    req = apply_static_headers(req, &server.static_headers, &vars);

    let tools = fetch_tools(req).await?;
    Ok(Json(ToolsResponse { server_id, tools }))
}

/// Build instance-scoped variables from server credentials (used by test_tools).
fn build_instance_vars(
    server: &McpServerRow,
    enc_key: Option<&str>,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let Some(key) = enc_key else { return m };
    let Some(vars_def) = server.mcp_info.get("variables").and_then(|v| v.as_array()) else {
        return m;
    };
    for var in vars_def {
        let name = match var.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        if var.get("scope").and_then(|v| v.as_str()) != Some("per_user") {
            if let Some(raw) = server.credentials.get(name).and_then(|v| v.as_str()) {
                let val = credential_crypto::decrypt_value(raw, key)
                    .unwrap_or_else(|_| raw.to_owned());
                m.insert(name.to_owned(), val);
            }
        }
    }
    m
}
