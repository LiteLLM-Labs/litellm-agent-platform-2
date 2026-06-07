use litellm_rust::db::managed_agents::{messages, sessions as db_sessions};
use serde_json::{json, Value};

use crate::support::{read_events_until_completed, request_json, AppFixture};

pub async fn exercise_platform_mcps(fixture: &AppFixture, agent_id: &str) {
    assert_catalog(fixture).await;
    assert_tools_list(fixture, agent_id).await;
    assert_memory_write(fixture, agent_id).await;
    assert_session_read(fixture, agent_id).await;
    assert_session_send(fixture, agent_id).await;
    super::platform_factory::assert_agent_factory(fixture, agent_id).await;
}

async fn assert_catalog(fixture: &AppFixture) {
    let catalog = request_json(fixture.app.clone(), "GET", "/api/platform-mcps", None).await;
    let ids: Vec<_> = catalog["platform_mcps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mcp| mcp["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "read_platform_session",
            "send_platform_session_message",
            "agent_memory",
            "send_slack_message",
            "create_managed_agent",
            "connect_agent_to_slack",
            "list_slack_agent_bindings"
        ]
    );
}

async fn assert_tools_list(fixture: &AppFixture, agent_id: &str) {
    let tools = rpc(
        fixture,
        agent_id,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    assert_eq!(
        tools["result"]["tools"][0]["name"],
        json!("read_platform_session")
    );
}

async fn assert_memory_write(fixture: &AppFixture, agent_id: &str) {
    let saved = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "agent_memory",
                "arguments": { "action": "set", "key": "platform", "value": "updated", "always_on": true }
            }
        }),
    )
    .await;
    assert!(content_text(&saved).contains("\"key\": \"platform\""));
}

async fn assert_session_read(fixture: &AppFixture, agent_id: &str) {
    let session_id = seed_session_message(fixture, agent_id).await;
    let read = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "read_platform_session",
                "arguments": { "session_id": session_id }
            }
        }),
    )
    .await;
    assert!(content_text(&read).contains("hello from session"));
}

async fn assert_session_send(fixture: &AppFixture, agent_id: &str) {
    let session_id = seed_empty_session(fixture, agent_id).await;
    let sent = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "send_platform_session_message",
                "arguments": { "session_id": session_id, "text": "continue from mcp" }
            }
        }),
    )
    .await;
    assert!(content_text(&sent).contains(&session_id));

    let events = read_events_until_completed(fixture.app.clone(), "/event", &session_id).await;
    assert!(events.contains("\"type\":\"session.idle\""));

    let read = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "read_platform_session",
                "arguments": { "session_id": session_id }
            }
        }),
    )
    .await;
    let content = content_text(&read);
    assert!(content.contains("continue from mcp"));
    assert!(content.contains("hello from managed agent"));
}

async fn seed_session_message(fixture: &AppFixture, agent_id: &str) -> String {
    let session = db_sessions::repository::create(
        &fixture.pool,
        "claude-code",
        Some(agent_id),
        "platform mcp test",
        None,
    )
    .await
    .unwrap();
    messages::repository::append(
        &fixture.pool,
        &session.id,
        &json!({"role": "user"}).to_string(),
        &json!([{"type": "text", "text": "hello from session"}]).to_string(),
    )
    .await
    .unwrap();
    session.id
}

async fn seed_empty_session(fixture: &AppFixture, agent_id: &str) -> String {
    db_sessions::repository::create(
        &fixture.pool,
        "claude-code",
        Some(agent_id),
        "platform mcp send test",
        None,
    )
    .await
    .unwrap()
    .id
}

async fn rpc(fixture: &AppFixture, agent_id: &str, body: Value) -> Value {
    request_json(
        fixture.app.clone(),
        "POST",
        &format!("/mcp/platform/{agent_id}"),
        Some(body),
    )
    .await
}

fn content_text(value: &Value) -> &str {
    value["result"]["content"][0]["text"].as_str().unwrap()
}
