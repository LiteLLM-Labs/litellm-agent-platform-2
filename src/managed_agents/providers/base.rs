use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    sdk::agents::{AgentRuntime, CLAUDE_MANAGED_AGENTS, CURSOR, OPENCODE},
};

pub const CURSOR_RUNTIME: &str = CURSOR;
pub const CLAUDE_AGENTS_RUNTIME: &str = CLAUDE_MANAGED_AGENTS;
pub const CLAUDE_AGENTS_RUNTIME_LEGACY: &str = "claude_agents";

#[derive(Debug, Clone)]
pub struct RuntimeCredential {
    pub api_key: String,
    pub api_base: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeSessionInput {
    pub session_id: String,
    pub prompt: String,
    pub environment: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProvision {
    pub runtime_agent_id: String,
    pub provider_session_id: Option<String>,
    pub provider_run_id: Option<String>,
    pub provider_url: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuntimeTool {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub enabled_by_default: bool,
}

pub fn validate_runtime(runtime: &str) -> bool {
    normalize_runtime(runtime).is_some()
}

pub fn normalize_runtime(runtime: &str) -> Option<&'static str> {
    match runtime {
        CLAUDE_AGENTS_RUNTIME | CLAUDE_AGENTS_RUNTIME_LEGACY => Some(CLAUDE_AGENTS_RUNTIME),
        CURSOR_RUNTIME => Some(CURSOR_RUNTIME),
        OPENCODE => Some(OPENCODE),
        _ => None,
    }
}

pub fn default_api_base(runtime: &str) -> Option<&'static str> {
    AgentRuntime::try_from(normalize_runtime(runtime)?)
        .ok()
        .map(AgentRuntime::default_api_base)
}

pub fn runtime_agent_id(agent: &ManagedAgentRow, runtime: &str) -> String {
    format!("{runtime}:{}", agent.id)
}

pub fn selected_tool_ids(
    tools: &Value,
    defaults: &'static [RuntimeTool],
    toolset_aliases: &[&str],
) -> Vec<String> {
    let Some(items) = tools.as_array() else {
        return default_tool_ids(defaults);
    };

    let mut values = items
        .iter()
        .filter_map(|value| selected_tool_id(value, defaults, toolset_aliases))
        .flatten()
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn selected_tool_id(
    value: &Value,
    defaults: &'static [RuntimeTool],
    toolset_aliases: &[&str],
) -> Option<Vec<String>> {
    let id = tool_id(value)?;
    if toolset_aliases.iter().any(|alias| *alias == id) {
        return Some(default_tool_ids(defaults));
    }
    defaults.iter().any(|tool| tool.id == id).then(|| vec![id])
}

fn default_tool_ids(defaults: &'static [RuntimeTool]) -> Vec<String> {
    defaults
        .iter()
        .filter(|tool| tool.enabled_by_default)
        .map(|tool| tool.id.to_owned())
        .collect()
}

fn tool_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value.get("type").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub fn toolset_payload(toolset_type: &str, selected: &[String]) -> Value {
    if selected.is_empty() {
        return json!([]);
    }
    json!([{
        "type": toolset_type,
        "default_config": { "enabled": false },
        "configs": selected
            .iter()
            .map(|name| json!({ "name": name, "enabled": true }))
            .collect::<Vec<_>>()
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOOLS: &[RuntimeTool] = &[
        RuntimeTool {
            id: "bash",
            name: "Shell",
            description: "Run commands.",
            enabled_by_default: true,
        },
        RuntimeTool {
            id: "read",
            name: "Read",
            description: "Read files.",
            enabled_by_default: true,
        },
        RuntimeTool {
            id: "web_search",
            name: "Web search",
            description: "Search the web.",
            enabled_by_default: false,
        },
    ];

    #[test]
    fn unset_tools_enable_defaults() {
        assert_eq!(
            selected_tool_ids(&Value::Null, TEST_TOOLS, &["agent_toolset_20260401"]),
            vec!["bash".to_owned(), "read".to_owned()]
        );
    }

    #[test]
    fn empty_tools_selects_no_tools() {
        assert!(selected_tool_ids(&json!([]), TEST_TOOLS, &["agent_toolset_20260401"]).is_empty());
    }

    #[test]
    fn legacy_toolset_alias_enables_defaults() {
        assert_eq!(
            selected_tool_ids(
                &json!([{ "type": "agent_toolset_20260401" }]),
                TEST_TOOLS,
                &["agent_toolset_20260401"]
            ),
            vec!["bash".to_owned(), "read".to_owned()]
        );
    }

    #[test]
    fn unknown_tool_ids_do_not_enable_defaults() {
        assert!(selected_tool_ids(&json!([{ "type": "unknown" }]), TEST_TOOLS, &[]).is_empty());
    }
}
