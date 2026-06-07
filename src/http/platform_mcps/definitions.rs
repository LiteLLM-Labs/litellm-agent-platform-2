use serde_json::{json, Value};

use super::{session_management, AGENT_MEMORY_MCP_ID, SEND_SLACK_MESSAGE_MCP_ID};

pub fn tool_defs() -> Vec<Value> {
    vec![
        session_management::read_tool_def(),
        session_management::send_tool_def(),
        agent_memory_tool(),
        send_slack_message_tool(),
    ]
}

fn agent_memory_tool() -> Value {
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
    })
}

fn send_slack_message_tool() -> Value {
    json!({
        "name": SEND_SLACK_MESSAGE_MCP_ID,
        "description": "Send a Slack channel message or DM using this agent's connected Slack bot.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "channel_id": {
                    "type": "string",
                    "description": "Slack channel ID, such as C123. When omitted, sends a DM."
                },
                "user_id": {
                    "type": "string",
                    "description": "Slack user ID, such as U123. Used for DMs only."
                },
                "email": {
                    "type": "string",
                    "description": "Slack user email. Used for DMs when user_id is omitted."
                },
                "text": { "type": "string" }
            },
            "required": ["text"]
        }
    })
}
