use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{
        header::{ACCEPT, CONTENT_TYPE},
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
    },
    response::Response,
};
use futures_util::TryStreamExt;

use crate::{
    db::{credentials, mcp_servers::repository},
    errors::GatewayError,
    proxy::{auth::master_key::require_any_gateway_key, credential_crypto, state::AppState},
};

use super::caller_user_id;

/// `GET|POST|PUT|DELETE|PATCH /{mcp_server_name}/mcp`
///
/// Proxies MCP protocol traffic to the registered upstream server, injecting
/// the calling user's credential (personal vault key, falling back to the
/// server's own stored credential).
pub async fn dynamic_mcp(
    State(state): State<Arc<AppState>>,
    Path(server_name): Path<String>,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Result<Response, GatewayError> {
    require_any_gateway_key(&headers, &state)?;

    // ── 1. Resolve server ─────────────────────────────────────────────────────
    let pool = state.db.as_ref().ok_or(GatewayError::MissingDatabase)?;
    let server = repository::get_by_name(pool, &server_name)
        .await?
        .ok_or_else(|| GatewayError::NotFound(format!("MCP server '{server_name}' not found")))?;

    // ── 2. Target URL ─────────────────────────────────────────────────────────
    let base_url = server
        .url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidJsonMessage("MCP server has no URL configured".to_owned())
        })?;

    // Forward to the server URL as-is; the registered URL is the full endpoint.
    let target_url = base_url.trim_end_matches('/').to_owned();

    // ── 3. Resolve credential ─────────────────────────────────────────────────
    let user_id = caller_user_id(&headers, &state);
    let enc_key =
        credential_crypto::encryption_key(state.config.general_settings.master_key.as_deref())?;

    let credential: Option<String> =
        resolve_user_credential(pool, &server.server_id, &user_id, &enc_key)
            .await?
            .or_else(|| resolve_server_credential(&server.credentials, &enc_key));

    // ── 4. Build outbound request ─────────────────────────────────────────────
    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|_| GatewayError::InvalidJsonMessage("invalid HTTP method".to_owned()))?;

    let mut req = state.http.request(reqwest_method, &target_url);

    // Forward safe inbound headers.
    for (name, value) in forward_headers(&headers) {
        req = req.header(name, value);
    }

    // Inject configured static headers (always sent, last to win on conflict).
    if let Some(obj) = server.static_headers.as_object() {
        for (name, val) in obj {
            if let Some(v) = val.as_str() {
                if let (Ok(n), Ok(hv)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(v),
                ) {
                    req = req.header(n, hv);
                }
            }
        }
    }

    // Inject auth header.
    if let Some(cred) = credential {
        req = apply_auth(req, server.auth_type.as_deref(), &cred);
    }

    if !body.is_empty() {
        req = req.body(body);
    }

    // ── 5. Stream response back ───────────────────────────────────────────────
    let upstream = req.send().await.map_err(GatewayError::Upstream)?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = copy_response_headers(upstream.headers());
    let stream = upstream.bytes_stream().map_err(std::io::Error::other);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    *response.headers_mut() = resp_headers;
    Ok(response)
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Look up the personal vault key for this (server, user) pair and decrypt it.
/// Key format: `mcp_user:{server_id}:{user_id}`
async fn resolve_user_credential(
    pool: &sqlx::PgPool,
    server_id: &str,
    user_id: &str,
    enc_key: &str,
) -> Result<Option<String>, GatewayError> {
    let key_name = format!("mcp_user:{}:{}", server_id, user_id);
    let Some(row) = credentials::get_personal_by_name(pool, &key_name, user_id).await? else {
        return Ok(None);
    };
    // credential_values is stored as { "value": "<encrypted>" }
    let Some(encrypted) = row
        .credential_values
        .as_object()
        .and_then(|m| m.get("value"))
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let plaintext = credential_crypto::decrypt_value(encrypted, enc_key)?;
    Ok(Some(plaintext))
}

/// Fall back to the server's own `credentials` JSONB field.
///
/// Supports two shapes:
/// - `{ "value": "<encrypted>" }` — decrypt with the platform key.
/// - `{ "api_key": "<plaintext>" }` — use as-is.
fn resolve_server_credential(credentials: &serde_json::Value, enc_key: &str) -> Option<String> {
    let obj = credentials.as_object()?;

    if let Some(encrypted) = obj.get("value").and_then(|v| v.as_str()) {
        // Best-effort decrypt; if decryption fails we skip this credential.
        return credential_crypto::decrypt_value(encrypted, enc_key).ok();
    }

    if let Some(api_key) = obj.get("api_key").and_then(|v| v.as_str()) {
        if !api_key.trim().is_empty() {
            return Some(api_key.to_owned());
        }
    }

    None
}

/// Apply the appropriate `Authorization` / `x-api-key` header based on auth_type.
fn apply_auth(
    req: reqwest::RequestBuilder,
    auth_type: Option<&str>,
    credential: &str,
) -> reqwest::RequestBuilder {
    match auth_type {
        Some("bearer_token") => req.header("Authorization", format!("Bearer {credential}")),
        Some("api_key") => req.header("x-api-key", credential),
        Some("basic") => req.header("Authorization", format!("Basic {credential}")),
        _ => req,
    }
}

/// Forward a safe subset of inbound request headers to the upstream.
fn forward_headers(headers: &HeaderMap) -> Vec<(HeaderName, HeaderValue)> {
    const CONNECT_PROTOCOL_VERSION: &str = "connect-protocol-version";

    let mut out = Vec::new();
    for name in [ACCEPT, CONTENT_TYPE] {
        if let Some(value) = headers.get(&name) {
            out.push((name, value.clone()));
        }
    }
    // Forward Connect-Protocol-Version (used by connect-rpc / MCP over HTTP).
    if let Some(value) = headers.get(CONNECT_PROTOCOL_VERSION) {
        if let Ok(name) = HeaderName::from_bytes(CONNECT_PROTOCOL_VERSION.as_bytes()) {
            out.push((name, value.clone()));
        }
    }
    out
}

/// Copy response headers that should be relayed to the caller.
fn copy_response_headers(headers: &reqwest::header::HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    let relay = [
        CONTENT_TYPE.as_str(),
        "cache-control",
        "connect-protocol-version",
        "connect-content-encoding",
    ];
    for name_str in relay {
        if let Some(value) = headers.get(name_str) {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(name_str.as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                out.insert(name, val);
            }
        }
    }
    out
}
