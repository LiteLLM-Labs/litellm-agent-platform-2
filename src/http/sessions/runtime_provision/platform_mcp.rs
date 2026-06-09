use serde_json::Value;

use crate::{
    errors::GatewayError,
    proxy::state::AppState,
    sdk::agents::{AgentRuntime, ANTHROPIC_VERSION, MANAGED_AGENTS_BETA},
};

use super::CreatedRuntimeSession;

pub(super) async fn vault_ids(
    state: &AppState,
    created: &CreatedRuntimeSession,
) -> Result<Option<Vec<String>>, GatewayError> {
    if created.resolved.agent_runtime != AgentRuntime::ClaudeManagedAgents {
        return Ok(None);
    }
    // Every gateway-hosted MCP URL Anthropic will call needs a vault credential
    // (a static bearer token bound to that URL) so the inbound call authenticates
    // against the gateway proxy: the platform MCP, plus each registered server we
    // rewrote to the proxy (e.g. Composio with per-user variables).
    let mut urls = Vec::new();
    if !crate::http::platform_mcps::selected_platform_mcp_ids(&created.agent.config).is_empty() {
        urls.push(crate::http::platform_mcps::platform_mcp_url(
            state,
            &created.agent.id,
            Some(&created.row.id),
        )?);
    }
    urls.extend(
        crate::http::sessions::runtime_mcp_validation::registered_mcp_proxy_urls(
            state,
            &created.agent.config,
        )?,
    );
    if urls.is_empty() {
        return Ok(None);
    }
    let token = state
        .config
        .general_settings
        .master_key
        .as_deref()
        .ok_or_else(|| {
            GatewayError::InvalidConfig("master_key is required for MCP vault auth".to_owned())
        })?;
    let api_key = &created.resolved.credential.api_key;
    let vault_id = create_vault(state, api_key).await?;
    for url in &urls {
        add_vault_credential(state, api_key, &vault_id, url, token).await?;
    }
    Ok(Some(vec![vault_id]))
}

async fn create_vault(state: &AppState, api_key: &str) -> Result<String, GatewayError> {
    let base = "https://api.anthropic.com/v1";
    let vault: Value = state
        .http
        .post(format!("{base}/vaults?beta=true"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&serde_json::json!({ "display_name": "LiteLLM gateway MCP" }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?
        .error_for_status()
        .map_err(GatewayError::Upstream)?
        .json()
        .await
        .map_err(GatewayError::Upstream)?;
    vault
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| GatewayError::SandboxError("Anthropic vault response missing id".to_owned()))
}

async fn add_vault_credential(
    state: &AppState,
    api_key: &str,
    vault_id: &str,
    mcp_server_url: &str,
    token: &str,
) -> Result<(), GatewayError> {
    let base = "https://api.anthropic.com/v1";
    let credential = state
        .http
        .post(format!("{base}/vaults/{vault_id}/credentials?beta=true"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&serde_json::json!({
            "auth": {
                "type": "static_bearer",
                "mcp_server_url": mcp_server_url,
                "token": token
            }
        }))
        .send()
        .await
        .map_err(GatewayError::Upstream)?;
    if !credential.status().is_success() {
        let status = credential.status();
        let body = credential.text().await.unwrap_or_default();
        return Err(GatewayError::SandboxError(format!(
            "Anthropic vault credential create failed with status {status}: {body}"
        )));
    }
    Ok(())
}
