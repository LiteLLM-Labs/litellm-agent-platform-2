use serde_json::Value;

use crate::errors::GatewayError;

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
