use serde_json::{json, Value};

use super::{
    factory, session_management, AGENT_MEMORY_MCP_ID, API_CALL_WITH_VAULT_MCP_ID,
    LIST_SUB_AGENTS_MCP_ID, RUN_SUB_AGENT_MCP_ID, SEND_SLACK_MESSAGE_MCP_ID,
};

pub fn tool_defs() -> Vec<Value> {
    let mut tools = vec![
        session_management::read_tool_def(),
        session_management::send_tool_def(),
        agent_memory_tool(),
        send_slack_message_tool(),
        list_sub_agents_tool(),
        run_sub_agent_tool(),
        api_call_with_vault_tool(),
    ];
    tools.extend(factory::tool_defs());
    tools
}

fn api_call_with_vault_tool() -> Value {
    json!({
        "name": API_CALL_WITH_VAULT_MCP_ID,
        "description": "Call a normal HTTPS API through LiteLLM's server-side vault credential proxy. The credential key must be attached to this agent. The secret is injected server-side and redacted from the returned response.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Attached vault key name to use for this request."
                },
                "url": {
                    "type": "string",
                    "description": "HTTPS URL to call. HTTP is only allowed for localhost during development."
                },
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"],
                    "description": "HTTP method. Defaults to GET."
                },
                "auth": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["bearer", "api_key_header", "header", "query"]
                        },
                        "header": {
                            "type": "string",
                            "description": "Header name for header or api_key_header auth. Defaults to x-api-key for api_key_header."
                        },
                        "query": {
                            "type": "string",
                            "description": "Query parameter name for query auth."
                        }
                    },
                    "required": ["type"]
                },
                "headers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "description": "Optional JSON body for POST, PUT, or PATCH."
                }
            },
            "required": ["key", "url"]
        }
    })
}

fn list_sub_agents_tool() -> Value {
    json!({
        "name": LIST_SUB_AGENTS_MCP_ID,
        "description": "List this parent agent's attached LAP sub-agents, including each agent_id, name, description, model, and runtime. Call this before run_sub_agent when choosing by name.",
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
}

fn run_sub_agent_tool() -> Value {
    json!({
        "name": RUN_SUB_AGENT_MCP_ID,
        "description": "Run one of this agent's configured LAP sub-agents. Only agent IDs attached to this parent agent are allowed. Use list_sub_agents first when you need the attached agents' names.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "The LAP agent ID from this parent agent's sub_agents list."
                },
                "prompt": {
                    "type": "string",
                    "description": "The complete task, context, paths, and expected output for the sub-agent."
                },
                "title": {
                    "type": "string",
                    "description": "Optional session title."
                }
            },
            "required": ["agent_id", "prompt"]
        }
    })
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
