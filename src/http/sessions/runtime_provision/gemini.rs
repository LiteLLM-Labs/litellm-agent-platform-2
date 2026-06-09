use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::managed_agents::runtime_refs,
    errors::GatewayError,
    http::agent_runtime_tools::runtime_tools,
    sdk::agents::{AgentRuntime, ManagedAgent},
};

use super::super::runtime_inputs::integration_mcp_toolsets;
use super::CreatedRuntimeSession;

pub(super) async fn reusable_provider_agent(
    pool: &PgPool,
    runtime: AgentRuntime,
    created: &CreatedRuntimeSession,
) -> Result<Option<ManagedAgent>, GatewayError> {
    if runtime != AgentRuntime::GeminiAntigravity {
        return Ok(None);
    }
    let Some(runtime_ref) =
        runtime_refs::repository::get(pool, &created.agent.id, &created.runtime).await?
    else {
        return Ok(None);
    };
    let raw = runtime_ref
        .metadata
        .get("agent")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "id": runtime_ref.runtime_agent_id }));
    Ok(Some(ManagedAgent {
        id: runtime_ref.runtime_agent_id,
        version: None,
        name: raw
            .get("display_name")
            .or_else(|| raw.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        model: raw
            .get("base_agent")
            .and_then(Value::as_str)
            .map(str::to_owned),
        system: raw
            .get("system_instruction")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tools: raw
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        mcp_servers: Vec::new(),
        metadata: None,
        created_at: None,
        updated_at: None,
        raw,
    }))
}

pub(super) fn provider_tools(runtime: AgentRuntime, created: &CreatedRuntimeSession) -> Vec<Value> {
    if runtime == AgentRuntime::GeminiAntigravity {
        return gemini_tools(created);
    }
    let mut tools = vec![serde_json::json!({ "type": "agent_toolset_20260401" })];
    tools.extend(crate::http::platform_mcps::platform_mcp_toolsets(
        &created.agent.config,
    ));
    tools.extend(integration_mcp_toolsets(&created.agent.config));
    tools
}

fn gemini_tools(created: &CreatedRuntimeSession) -> Vec<Value> {
    let tools: Vec<Value> = created
        .agent
        .tools
        .as_array()
        .or_else(|| created.agent.config.get("tools").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter(|tool| {
            tool.get("type")
                .and_then(Value::as_str)
                .is_some_and(|tool_type| {
                    matches!(
                        tool_type,
                        "code_execution" | "google_search" | "url_context"
                    )
                })
        })
        .cloned()
        .collect();
    if !tools.is_empty() {
        return tools;
    }
    runtime_tools(crate::sdk::agents::GEMINI_ANTIGRAVITY)
        .iter()
        .filter(|tool| tool.enabled_by_default)
        .map(|tool| serde_json::json!({ "type": tool.id }))
        .collect()
}
