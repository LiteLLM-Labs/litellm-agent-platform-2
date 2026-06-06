# Cursor Managed Agents SDK

This page shows how to use the Rust managed-agents SDK with the Cursor runtime.
The SDK keeps an Anthropic-shaped public contract and translates only the fields
that Cursor can support.

## Configure

```rust
use litellm_rust::sdk::agents::{Lap, LapConfig};

let client = Lap::new(LapConfig::cursor("cursor-api-key"));
```

For tests or local tools, prefer reading the key from the environment:

```rust
let client = Lap::new(LapConfig::cursor(std::env::var("CURSOR_API_KEY")?));
```

## Create A Cursor Agent

Cursor Cloud Agents create a durable agent and enqueue an initial run in the
same provider call. The SDK maps `CreateAgentParams.system` to Cursor
`prompt.text`.

```rust
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, Lap, LapConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Lap::new(LapConfig::cursor(std::env::var("CURSOR_API_KEY")?));

    let agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::Cursor,
            name: "Coding Assistant".to_owned(),
            model: AgentModel::from("default"),
            system: "Inspect the project and reply with a short status.".to_owned(),
            description: None,
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            metadata: None,
        })
        .await?;

    println!("Cursor agent ID: {}", agent.id);
    Ok(())
}
```

The returned `agent.id` is the Cursor runtime agent ID, usually `bc-...`.

## Create A Session Handle

Cursor does not expose an Anthropic-style session resource. In this SDK,
`sessions.create` validates and records local routing context for a Cursor
runtime agent ID.

```rust
use litellm_rust::sdk::agents::{AgentRuntime, CreateSessionParams};

let session = client
    .beta()
    .sessions()
    .create(CreateSessionParams {
        agent: agent.id.into(),
        environment_id: String::new(),
        title: "Cursor session".to_owned(),
        lap_agent_runtime: Some(AgentRuntime::Cursor),
        metadata: None,
    })
    .await?;
```

`session.id` is the Cursor agent ID. It is not a LAP database ID.

## Send A Follow-Up Run

`events.send` converts a strict `user.message` event into a Cursor follow-up run:

```rust
use litellm_rust::sdk::agents::{SendEventsParams, UserEvent};

client
    .beta()
    .sessions()
    .events()
    .send(
        &session.id,
        SendEventsParams {
            events: vec![UserEvent::text("Add a troubleshooting section.")],
        },
    )
    .await?;
```

## Stream Events

Cursor SSE events are normalized into SDK event names:

```rust
use futures_util::StreamExt;

let mut stream = client
    .beta()
    .sessions()
    .events()
    .stream(&session.id)
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
        "agent.tool_use" => println!("[tool: {:?}]", event.data.get("name")),
        "session.status_idle" => break,
        "session.error" => return Err(format!("Cursor run failed: {:?}", event.data).into()),
        _ => {}
    }
}
```

## MCP Servers

The strict SDK contract accepts Anthropic-shaped MCP servers:

```rust
use litellm_rust::sdk::agents::{
    ManagedAgentMcpServer, ManagedAgentMcpServerType,
};

mcp_servers: vec![ManagedAgentMcpServer {
    name: "linear".to_owned(),
    server_type: ManagedAgentMcpServerType::Url,
    url: "https://mcp.linear.app/sse".to_owned(),
}],
```

For Cursor, the provider adapter translates this to `mcpServers` with
`type: "http"` on agent creation.

## Supported Params

Use the inspection helpers to see what translates for Cursor:

```rust
let params = client.supported_managed_agents_create_agent_params(AgentRuntime::Cursor)?;
assert_eq!(params, &["name", "model", "system", "mcp_servers"]);
```

Current Cursor support:

```text
create_agent:        name, model, system, mcp_servers
create_environment:  none
create_session:      agent
send_events:         events
```

The SDK intentionally does not accept Cursor-specific session `resources`,
`envVars`, or `repos`. LAP orchestration and vault/database code live outside
this provider SDK.

## Live Smoke Test

The repo includes an ignored live test:

```bash
CURSOR_API_KEY=... cargo test --test cursor_live_e2e -- --ignored --nocapture
```

Optional:

```bash
CURSOR_MODEL=claude-sonnet-4-6 CURSOR_API_KEY=... \
  cargo test --test cursor_live_e2e -- --ignored --nocapture
```

The live test creates a real Cursor agent through the SDK, streams through the
SDK abstraction, prints the final response, and archives the created agent.
