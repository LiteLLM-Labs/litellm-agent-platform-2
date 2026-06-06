# LAP Managed Agents SDK Contract

This SDK is a provider-facing Rust client for managed-agent runtimes. It does
not read or write LAP database state. LAP service code owns DB lookup,
idempotency, vaults, and remote-ID storage.

## Rust Quickstart

```rust
use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Lap, LapConfig, SendEventsParams,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Lap::new(LapConfig::anthropic("sk-ant-..."));

    let agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            name: "Coding Assistant".to_owned(),
            model: AgentModel::from("claude-opus-4-8"),
            system: "You are a helpful coding assistant. Write clean, well-documented code."
                .to_owned(),
            description: None,
            tools: vec![json!({ "type": "agent_toolset_20260401" })],
            mcp_servers: Vec::new(),
        })
        .await?;

    println!("Agent ID: {}, version: {:?}", agent.id, agent.version);

    let environment = client
        .beta()
        .environments()
        .create(CreateEnvironmentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            name: "quickstart-env".to_owned(),
            config: json!({
                "type": "cloud",
                "networking": { "type": "unrestricted" },
            }),
            description: None,
            scope: None,
        })
        .await?;

    println!("Environment ID: {}", environment.id);

    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: agent.id,
            environment_id: environment.id,
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: None,
            metadata: None,
            resources: None,
        })
        .await?;

    println!("Session ID: {}", session.id);

    let mut stream = client
        .beta()
        .sessions()
        .events()
        .stream(&session.id)
        .await?;

    client
        .beta()
        .sessions()
        .events()
        .send(
            &session.id,
            SendEventsParams {
                events: vec![json!({
                    "type": "user.message",
                    "content": [{
                        "type": "text",
                        "text": "Create a Python script that generates the first 20 Fibonacci numbers and saves them to fibonacci.txt"
                    }]
                })],
            },
        )
        .await?;

    while let Some(event) = stream.next().await {
        let event = event?;
        match event.event_type.as_str() {
            "agent.message" => {
                if let Some(content) = event.data.get("content").and_then(|value| value.as_array()) {
                    for block in content {
                        if let Some(text) = block.get("text").and_then(|value| value.as_str()) {
                            print!("{text}");
                        }
                    }
                }
            }
            "agent.tool_use" => {
                if let Some(name) = event.data.get("name").and_then(|value| value.as_str()) {
                    println!("\n[Using tool: {name}]");
                }
            }
            "session.status_idle" => {
                println!("\n\nAgent finished.");
                break;
            }
            "session.error" => return Err(format!("session error: {:?}", event.data).into()),
            _ => {}
        }
    }

    Ok(())
}
```

## Contract

- `LapConfig::anthropic(...)` configures the `claude_managed_agents` runtime.
- `LapConfig::cursor(...)` configures the `cursor` runtime.
- `lap_agent_runtime` is the only LAP-specific create parameter.
- `agent.id`, `environment.id`, and `session.id` are provider/runtime IDs.
- The SDK forwards Anthropic-shaped payloads to the runtime provider.
- Cursor requests are translated to Cursor's v1 Cloud Agent APIs.
- The SDK sets the Managed Agents beta header and parses SSE session events.
- Cursor stream chunks are normalized to Anthropic Managed Agents event shape.
- The gateway exposes the same normalized stream at `/v1/sessions/{session_id}/events/stream`.
- The SDK does not perform DB calls, idempotency checks, or vault operations.

## Supported Surface

```rust
client.beta().agents().create(...)
client.beta().environments().create(...)
client.beta().sessions().create(...)
client.beta().sessions().events().send(...)
client.beta().sessions().events().stream(...)
```

Implemented runtimes:

```text
claude_managed_agents
cursor
```

## Future Python Sugar

Python bindings can wrap this Rust client and preserve the Anthropic-like shape:

```python
from lap import LAP

client = LAP(anthropic_api_key="sk-ant-...")

agent = client.beta.agents.create(
    lap_agent_runtime="claude_managed_agents",
    name="Coding Assistant",
    model="claude-opus-4-8",
    system="You are a helpful coding assistant.",
    tools=[{"type": "agent_toolset_20260401"}],
)
```

The Python API is sugar over the Rust SDK contract, not the source of truth.
