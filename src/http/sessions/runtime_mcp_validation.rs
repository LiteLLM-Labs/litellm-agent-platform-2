use serde_json::Value;

use crate::{errors::GatewayError, proxy::state::AppState};

/// Build this gateway's MCP proxy URL for a registered server `name` (id/alias).
fn mcp_proxy_url(base: &str, name: &str) -> String {
    format!("{}/{}/mcp", base.trim_end_matches('/'), name)
}

/// Registered MCP servers attached to an agent store the *raw* upstream URL,
/// which may carry `${VAR}` placeholders resolved per-user at call time (e.g.
/// Composio's `${COMPOSIO_USER_ID}`). The managed-agents runtime (Anthropic)
/// calls the MCP URL directly and rejects an unresolved `${...}` URL as an
/// invalid URI. So any templated entry is rewritten to route through this
/// gateway's MCP proxy (`{proxy_base}/{name}/mcp`), which resolves the caller's
/// vault variables, injects static headers, and forwards upstream — the same
/// path tool discovery uses. Inbound auth to the proxy is provided to Anthropic
/// via a vault credential bound to this URL (see `platform_mcp::vault_ids`), NOT
/// an `authorization_token` field (which the managed-agents API rejects).
/// `name` is the server id; the dynamic proxy resolves it by id, name, or alias.
///
/// v0: on-behalf-of identity is the default owner. Per-user identity over this
/// path is tracked separately (signed-token auth) — see issue.
pub(super) fn rewrite_registered_mcp_servers(
    state: &AppState,
    servers: &mut [Value],
) -> Result<(), GatewayError> {
    for server in servers.iter_mut() {
        let Some(obj) = server.as_object_mut() else {
            continue;
        };
        if !obj
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("${"))
        {
            continue;
        }
        let name = registered_server_name(obj)?;
        let base = proxy_base_url(state)?;
        obj.insert("url".to_owned(), Value::String(mcp_proxy_url(&base, &name)));
        // Auth is supplied via a vault credential bound to this URL, not an
        // inline token — drop any token the entry carried (it targets upstream).
        obj.remove("authorization_token");
    }
    Ok(())
}

/// The proxy URLs that registered (templated) MCP servers were rewritten to.
/// Used to mint vault credentials so Anthropic can authenticate inbound calls.
pub(super) fn registered_mcp_proxy_urls(
    state: &AppState,
    config: &Value,
) -> Result<Vec<String>, GatewayError> {
    let Some(servers) = config
        .get("mcp_servers")
        .or_else(|| config.get("mcpServers"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut urls = Vec::new();
    for server in servers {
        let Some(obj) = server.as_object() else {
            continue;
        };
        if !obj
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("${"))
        {
            continue;
        }
        let name = registered_server_name(obj)?;
        urls.push(mcp_proxy_url(&proxy_base_url(state)?, &name));
    }
    Ok(urls)
}

fn registered_server_name(obj: &serde_json::Map<String, Value>) -> Result<String, GatewayError> {
    obj.get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            GatewayError::InvalidConfig(
                "mcp_servers entry with ${variables} requires a name (server id)".to_owned(),
            )
        })
}

fn proxy_base_url(state: &AppState) -> Result<String, GatewayError> {
    state.resolved_mcp_proxy_base_url().ok_or_else(|| {
        GatewayError::InvalidConfig(
            "mcp_servers.proxy_base_url is required to proxy MCP servers with variables".to_owned(),
        )
    })
}

pub(super) fn validate_runtime_mcp_servers(
    agent_id: &str,
    servers: &[Value],
) -> Result<(), GatewayError> {
    for (index, server) in servers.iter().enumerate() {
        let Some(server) = server.as_object() else {
            return Err(GatewayError::InvalidConfig(format!(
                "{agent_id} config.mcp_servers.{index} must be an object"
            )));
        };
        let server_type = server.get("type").and_then(Value::as_str).unwrap_or("url");
        if server_type != "url" {
            continue;
        }
        let Some(url) = server
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            return Err(GatewayError::InvalidConfig(format!(
                "{agent_id} config.mcp_servers.{index}.url is required"
            )));
        };
        // Reject unresolved `${VAR}` placeholders: reqwest::Url::parse tolerates
        // them (percent-encoding), but the managed-agents runtime rejects such a
        // URL as an invalid URI. Templated registered servers must have been
        // rewritten to the gateway proxy by `rewrite_registered_mcp_servers`.
        if url.contains("${") {
            return Err(GatewayError::InvalidConfig(format!(
                "{agent_id} config.mcp_servers.{index}.url contains unresolved variables; \
                 it must be proxied through the gateway or fully resolved"
            )));
        }
        let parsed = reqwest::Url::parse(url).map_err(|_| {
            GatewayError::InvalidConfig(format!(
                "{agent_id} config.mcp_servers.{index}.url must be an absolute http(s) URL"
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(GatewayError::InvalidConfig(format!(
                "{agent_id} config.mcp_servers.{index}.url must be an absolute http(s) URL"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_runtime_mcp_servers;

    #[test]
    fn validates_runtime_mcp_server_urls() {
        let err = validate_runtime_mcp_servers(
            "agent_1",
            &[json!({
                "name": "gmail",
                "type": "url",
                "url": "gmail"
            })],
        )
        .unwrap_err()
        .to_string();

        assert!(
            err.contains("agent_1 config.mcp_servers.0.url must be an absolute http(s) URL"),
            "got: {err}"
        );

        validate_runtime_mcp_servers(
            "agent_1",
            &[json!({
                "name": "gmail",
                "type": "url",
                "url": "https://mcp.composio.dev/gmail"
            })],
        )
        .unwrap();
    }
}
