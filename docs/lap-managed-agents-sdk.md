# LAP Managed Agents SDK Contract

This SDK is a provider-facing Rust client for managed-agent runtimes. It does
not read or write LAP database state. LAP service code owns DB lookup,
idempotency, vaults, and remote-ID storage.

## Rust Quickstart

```rust
use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, EnvironmentConfig, EnvironmentNetworking, Lap, LapConfig,
    ManagedAgentTool, SendEventsParams, UserEvent,
};

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
            tools: vec![ManagedAgentTool::AgentToolset20260401],
            mcp_servers: Vec::new(),
            metadata: None,
        })
        .await?;

    println!("Agent ID: {}, version: {:?}", agent.id, agent.version);

    let environment = client
        .beta()
        .environments()
        .create(CreateEnvironmentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            name: "quickstart-env".to_owned(),
            config: EnvironmentConfig::Cloud {
                networking: EnvironmentNetworking::Unrestricted,
            },
            description: None,
            scope: None,
            metadata: None,
        })
        .await?;

    println!("Environment ID: {}", environment.id);

    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: agent.id.into(),
            environment_id: environment.id,
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: None,
            metadata: None,
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
                events: vec![UserEvent::text(
                    "Create a Python script that generates the first 20 Fibonacci numbers and saves them to fibonacci.txt",
                )],
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
- Request params are strict Rust structs based on the Anthropic Managed Agents
  surface, not provider-specific JSON bags.
- The SDK sets the Managed Agents beta header and parses SSE session events.
- The SDK does not perform DB calls, idempotency checks, or vault operations.
- The SDK does not accept Cursor `envVars`, `repos`, or session `resources`.
  Those are not part of the strict Anthropic-shaped contract.

## Supported Surface

```rust
client.beta().agents().create(...)
client.beta().environments().create(...)
client.beta().sessions().create(...)
client.beta().sessions().events().send(...)
client.beta().sessions().events().stream(...)
```

`claude_managed_agents` and `cursor` are implemented in this slice.
`cursor` uses Cursor Cloud Agents API v1. Cursor does not have
the same pre-created agent/environment/session split as Anthropic: creating a
Cursor cloud agent enqueues a run. The SDK keeps the public contract strict:
Cursor `sessions.create` requires a Cursor runtime agent ID returned by
`agents.create`, records local routing context, and does not accept provider
escape hatches.

## Contract Inspection

Providers expose explicit contract helpers:

```rust
client.supported_managed_agents_create_agent_params(AgentRuntime::Cursor)?;
client.transform_managed_agents_create_agent_params(params)?;

client.supported_managed_agents_create_environment_params(AgentRuntime::Cursor)?;
client.transform_managed_agents_create_environment_params(params)?;

client.supported_managed_agents_create_session_params(AgentRuntime::Cursor)?;
client.transform_managed_agents_create_session_params(params)?;

client.supported_managed_agents_send_events_params(AgentRuntime::Cursor)?;
client.transform_managed_agents_send_events_params(AgentRuntime::Cursor, params)?;
```

Current supported param sets:

```text
Claude create agent:        name, model, system, description, tools, mcp_servers, metadata
Claude create environment:  name, config, description, scope, metadata
Claude create session:      agent, environment_id, title, metadata
Claude send events:         events

Cursor create agent:        name, model, system, mcp_servers
Cursor create environment:  none
Cursor create session:      agent
Cursor send events:         events
```

## Provider Layout

```text
src/sdk/agents/
  client.rs
  events.rs
  mod.rs
  types.rs

  providers/
    mod.rs
    transform.rs

    claude_managed_agents/
      mod.rs
      transformation.rs

    cursor/
      mod.rs
      transformation.rs
```

`client.rs` owns the Anthropic-like facade. `providers/transform.rs` owns the
runtime-provider trait and registry. Each provider folder owns its endpoint
mapping and event normalization.

## Cursor Session Example

```rust
use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, CreateSessionParams, Lap, LapConfig,
    SendEventsParams, UserEvent,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Lap::new(LapConfig::cursor("cursor-api-key"));

    let agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::Cursor,
            name: "Coding Assistant".to_owned(),
            model: AgentModel::from("composer-2"),
            system: "You are a helpful coding assistant.".to_owned(),
            description: None,
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            metadata: None,
        })
        .await?;

    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: agent.id.into(),
            environment_id: "quickstart-env".to_owned(),
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: Some(AgentRuntime::Cursor),
            metadata: None,
        })
        .await?;

    client
        .beta()
        .sessions()
        .events()
        .send(
            &session.id,
            SendEventsParams {
                events: vec![UserEvent::text("Also add a troubleshooting note")],
            },
        )
        .await?;

    let mut stream = client
        .beta()
        .sessions()
        .events()
        .stream(&session.id)
        .await?;

    while let Some(event) = stream.next().await {
        let event = event?;
        if event.event_type == "session.status_idle" {
            break;
        }
    }

    Ok(())
}
```

## Future Python Sugar

Python bindings can wrap this Rust client and preserve the Anthropic-like shape:

```python
from lap import LAP

client = LAP(anthropic_api_key="sk-ant-...")
cursor_client = LAP(cursor_api_key="cursor-api-key")

agent = client.beta.agents.create(
    lap_agent_runtime="claude_managed_agents",
    name="Coding Assistant",
    model="claude-opus-4-8",
    system="You are a helpful coding assistant.",
    tools=[{"type": "agent_toolset_20260401"}],
)
```

The Python API is sugar over the Rust SDK contract, not the source of truth.
