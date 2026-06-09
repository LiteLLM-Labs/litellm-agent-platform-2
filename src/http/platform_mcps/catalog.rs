use serde::Serialize;

use super::{
    AGENT_MEMORY_MCP_ID, API_CALL_WITH_VAULT_MCP_ID, CONNECT_AGENT_TO_SLACK_MCP_ID,
    CREATE_MANAGED_AGENT_MCP_ID, LIST_SLACK_AGENT_BINDINGS_MCP_ID, LIST_SUB_AGENTS_MCP_ID,
    PLATFORM_SESSION_MCP_ID, RUN_SUB_AGENT_MCP_ID, SEND_PLATFORM_SESSION_MESSAGE_MCP_ID,
    SEND_SLACK_MESSAGE_MCP_ID,
};

#[derive(Debug, Clone, Serialize)]
pub struct PlatformMcp {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

const PLATFORM_MCPS: [PlatformMcp; 10] = [
    entry(
        PLATFORM_SESSION_MCP_ID,
        "Read platform session",
        "Read persisted platform session messages for debugging and handoff.",
    ),
    entry(
        SEND_PLATFORM_SESSION_MESSAGE_MCP_ID,
        "Send platform session message",
        "Send a user message into a platform session and resume that agent run.",
    ),
    entry(
        AGENT_MEMORY_MCP_ID,
        "Read/Write agent memory",
        "List, read, and update DB-backed memory for a platform agent.",
    ),
    entry(
        SEND_SLACK_MESSAGE_MCP_ID,
        "Send Slack message",
        "Send a channel message or DM from this agent's connected Slack bot.",
    ),
    entry(
        CREATE_MANAGED_AGENT_MCP_ID,
        "Create managed agent",
        "Create a Claude managed agent from a Slack or platform request.",
    ),
    entry(
        CONNECT_AGENT_TO_SLACK_MCP_ID,
        "Connect agent to Slack",
        "Create a dedicated Slack app for a managed agent and return its install URL.",
    ),
    entry(
        LIST_SLACK_AGENT_BINDINGS_MCP_ID,
        "List Slack agent bindings",
        "List channel bindings created by this platform agent factory.",
    ),
    entry(
        LIST_SUB_AGENTS_MCP_ID,
        "List sub-agents",
        "List this agent's attached LAP sub-agents with IDs, names, and runtime.",
    ),
    entry(
        RUN_SUB_AGENT_MCP_ID,
        "Run sub-agent",
        "Run one of this agent's explicitly attached LAP sub-agents and return its session.",
    ),
    entry(
        API_CALL_WITH_VAULT_MCP_ID,
        "API call with vault",
        "Call an HTTPS API with an attached vault credential without exposing the credential to the agent.",
    ),
];

pub fn platform_mcps() -> Vec<PlatformMcp> {
    PLATFORM_MCPS.to_vec()
}

const fn entry(id: &'static str, name: &'static str, description: &'static str) -> PlatformMcp {
    PlatformMcp {
        id,
        name,
        description,
    }
}
