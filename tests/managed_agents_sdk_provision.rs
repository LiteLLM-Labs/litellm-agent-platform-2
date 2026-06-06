// Live e2e: agents.create → environments.create → sessions.create → send → stream.
// Runs for every runtime whose key is set in the environment.
// Same assertions hold for both providers.
//
// ANTHROPIC_API_KEY  — run against Claude Managed Agents
// CURSOR_API_KEY     — run against Cursor
//
// cargo test --test managed_agents_sdk_provision -- --ignored

use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    AgentEventKind, AgentModel, AgentModelConfig, AgentRuntime, CreateAgentParams,
    CreateEnvironmentParams, CreateSessionParams, Lap, LapConfig, SendEventsParams,
};
use serde_json::json;

struct RuntimeUnderTest {
    name: &'static str,
    runtime: AgentRuntime,
    lap: Lap,
    model: &'static str,
    /// Cursor embeds the initial prompt in agents.create and starts running
    /// immediately; calling send_events before that run completes returns 409.
    /// Set false to stream the initial run directly instead.
    needs_send_events: bool,
}

fn configured_runtimes() -> Vec<RuntimeUnderTest> {
    let mut runtimes = Vec::new();

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        runtimes.push(RuntimeUnderTest {
            name: "claude",
            runtime: AgentRuntime::ClaudeManagedAgents,
            lap: Lap::new(LapConfig::anthropic(key)),
            model: "claude-sonnet-4-6",
            needs_send_events: true,
        });
    }

    if let Ok(key) = std::env::var("CURSOR_API_KEY") {
        runtimes.push(RuntimeUnderTest {
            name: "cursor",
            runtime: AgentRuntime::Cursor,
            lap: Lap::new(LapConfig::cursor(key)),
            model: "default",
            needs_send_events: false, // agents.create already starts the initial run
        });
    }

    runtimes
}

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY and/or CURSOR_API_KEY"]
async fn provision_flow_returns_ids_and_streams_one_event() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let runtimes = configured_runtimes();
    assert!(
        !runtimes.is_empty(),
        "set ANTHROPIC_API_KEY or CURSOR_API_KEY to run this test"
    );

    for rt in runtimes {
        eprintln!("\n=== {} ===", rt.name);

        // 1. agents.create
        let agent = rt
            .lap
            .beta()
            .agents()
            .create(CreateAgentParams {
                lap_agent_runtime: rt.runtime,
                lap_provider_options: None,
                name: format!("lap-sdk-provision-test-{suffix}"),
                model: AgentModel::Config(AgentModelConfig {
                    id: rt.model.to_owned(),
                    speed: None,
                }),
                system: "Reply with exactly one word: ok".to_owned(),
                description: None,
                tools: vec![json!({ "type": "agent_toolset_20260401" })],
                mcp_servers: Vec::new(),
                workspace: None,
                env_vars: None,
                metadata: None,
            })
            .await
            .unwrap_or_else(|e| panic!("[{}] agents.create failed: {e}", rt.name));

        assert!(!agent.id.is_empty(), "[{}] agent.id is empty", rt.name);
        eprintln!("[{}] agent.id = {}", rt.name, agent.id);

        // 2. environments.create
        let env = rt
            .lap
            .beta()
            .environments()
            .create(CreateEnvironmentParams {
                lap_agent_runtime: rt.runtime,
                name: format!("lap-sdk-env-{suffix}"),
                config: json!({ "type": "cloud", "networking": { "type": "unrestricted" } }),
                description: None,
                scope: None,
            })
            .await
            .unwrap_or_else(|e| panic!("[{}] environments.create failed: {e}", rt.name));

        assert!(!env.id.is_empty(), "[{}] environment.id is empty", rt.name);
        eprintln!("[{}] environment.id = {}", rt.name, env.id);

        // 3. sessions.create
        let session = rt
            .lap
            .beta()
            .sessions()
            .create(CreateSessionParams {
                agent: agent.id.clone(),
                environment_id: env.id.clone(),
                title: format!("lap-sdk-session-{suffix}"),
                lap_agent_runtime: Some(rt.runtime),
                metadata: None,
                resources: None,
            })
            .await
            .unwrap_or_else(|e| panic!("[{}] sessions.create failed: {e}", rt.name));

        assert!(!session.id.is_empty(), "[{}] session.id is empty", rt.name);
        eprintln!("[{}] session.id = {}", rt.name, session.id);

        // 4. send a prompt (Claude only — Cursor's initial run starts on agents.create)
        if rt.needs_send_events {
            rt.lap
                .beta()
                .sessions()
                .events()
                .send(
                    &session.id,
                    SendEventsParams {
                        events: vec![json!({
                            "type": "user.message",
                            "content": [{ "type": "text", "text": "ok" }]
                        })],
                    },
                )
                .await
                .unwrap_or_else(|e| panic!("[{}] send_events failed: {e}", rt.name));
        }

        // 5. stream: expect at least one agent.message before session goes idle
        let mut stream = rt
            .lap
            .beta()
            .sessions()
            .events()
            .stream(&session.id)
            .await
            .unwrap_or_else(|e| panic!("[{}] stream failed: {e}", rt.name));

        let mut saw_message = false;
        while let Some(event) = stream.next().await {
            let event = event.unwrap_or_else(|e| panic!("[{}] stream error: {e}", rt.name));
            eprintln!("[{}] event: {}", rt.name, event.event_type);
            if event.kind() == AgentEventKind::AgentMessage {
                saw_message = true;
            }
            if event.kind() == AgentEventKind::SessionStatusIdle {
                break;
            }
        }

        assert!(
            saw_message,
            "[{}] never received agent.message event",
            rt.name
        );
    }
}
