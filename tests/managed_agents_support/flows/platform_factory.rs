use serde_json::{json, Value};

use crate::support::{request_json, AppFixture};

use super::slack_helpers::{now_seconds, signed_json_request};

pub async fn assert_agent_factory(fixture: &AppFixture, platform_agent_id: &str) {
    let _anthropic = super::claude_runtime::save_anthropic_credentials(fixture).await;
    save_platform_slack(fixture, platform_agent_id, "connected").await;
    let child_id = create_child_agent(fixture, platform_agent_id).await;
    connect_child_agent(fixture, platform_agent_id, &child_id).await;
    assert_factory_slack_dispatch(fixture, platform_agent_id, &child_id).await;
    assert_pending_install_url(fixture, platform_agent_id, &child_id).await;
}

async fn create_child_agent(fixture: &AppFixture, platform_agent_id: &str) -> String {
    let created = rpc(
        fixture,
        platform_agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "create_managed_agent",
                "arguments": {
                    "name": "Release Buddy",
                    "instructions": "Answer release questions from Slack.",
                    "owner_id": "slack:U123"
                }
            }
        }),
    )
    .await;
    let created = content_json(&created);
    assert_eq!(created["agent"]["harness"], "claude_managed_agents");
    assert_eq!(
        created["agent"]["config"]["runtime"],
        "claude_managed_agents"
    );
    created["agent"]["id"].as_str().unwrap().to_owned()
}

async fn connect_child_agent(fixture: &AppFixture, platform_agent_id: &str, child_id: &str) {
    let connected = rpc(
        fixture,
        platform_agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "connect_agent_to_slack",
                "arguments": {
                    "agent_id": child_id,
                    "team_id": "T123",
                    "channel_id": "C-factory",
                    "requested_by": "U123"
                }
            }
        }),
    )
    .await;
    let connected = content_json(&connected);
    assert_eq!(connected["status"], "connected");
    assert_eq!(connected["binding"]["agent_id"], child_id);
    assert!(listed_bindings(fixture, platform_agent_id)
        .await
        .contains("C-factory"));
}

async fn listed_bindings(fixture: &AppFixture, platform_agent_id: &str) -> String {
    let listed = rpc(
        fixture,
        platform_agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "list_slack_agent_bindings", "arguments": {} }
        }),
    )
    .await;
    content_text(&listed).to_owned()
}

async fn save_platform_slack(fixture: &AppFixture, agent_id: &str, status: &str) {
    for (key, value) in [
        (format!("SLACK_{agent_id}_SIGNING_SECRET"), "slack-secret"),
        (format!("SLACK_{agent_id}_CLIENT_SECRET"), "client-secret"),
        (format!("SLACK_{agent_id}_BOT_TOKEN"), "xoxb-test"),
    ] {
        request_json(
            fixture.app.clone(),
            "POST",
            "/api/vault/default",
            Some(json!({ "key": key, "value": value })),
        )
        .await;
    }
    request_json(
        fixture.app.clone(),
        "PATCH",
        &format!("/api/agents/{agent_id}"),
        Some(slack_config(agent_id, status)),
    )
    .await;
}

async fn assert_factory_slack_dispatch(
    fixture: &AppFixture,
    platform_agent_id: &str,
    child_agent_id: &str,
) {
    signed_json_request(
        fixture,
        &format!("/api/agents/{platform_agent_id}/slack/events"),
        child_message_body(),
        axum::http::StatusCode::OK,
    )
    .await;
    wait_for_child_thread(fixture, child_agent_id).await;
}

async fn wait_for_child_thread(fixture: &AppFixture, child_agent_id: &str) {
    for _ in 0..20 {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM "LiteLLM_ManagedAgentSlackThreadSessionsTable"
            WHERE agent_id = $1 AND channel_id = 'C-factory'
            "#,
        )
        .bind(child_agent_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        if count == 1 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("factory Slack event did not dispatch to child agent");
}

async fn assert_pending_install_url(
    fixture: &AppFixture,
    platform_agent_id: &str,
    child_agent_id: &str,
) {
    save_platform_slack(fixture, platform_agent_id, "needs_install").await;
    let install = rpc(fixture, platform_agent_id, install_call(child_agent_id)).await;
    let install = content_json(&install);
    assert_eq!(install["status"], "install_required");
    let install_url = install["install_url"].as_str().unwrap();
    assert!(install_url.starts_with("https://slack.com/oauth/v2/authorize?"));
    assert!(install_url.contains("redirect_uri=http%3A%2F%2Flocalhost%2Fhost-oauth-callback"));
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

fn slack_config(agent_id: &str, status: &str) -> Value {
    json!({
        "config": {
            "slack": {
                "status": status,
                "client_id": "client-id",
                "client_secret_key": format!("SLACK_{agent_id}_CLIENT_SECRET"),
                "signing_secret_key": format!("SLACK_{agent_id}_SIGNING_SECRET"),
                "bot_token_key": format!("SLACK_{agent_id}_BOT_TOKEN")
            }
        }
    })
}

fn child_message_body() -> String {
    json!({
        "type": "event_callback",
        "team_id": "T123",
        "api_app_id": "A123",
        "event_id": "Ev-factory-child",
        "event_time": now_seconds(),
        "event": {
            "type": "app_mention",
            "user": "U123",
            "text": "<@B123> what should I ship?",
            "ts": "1712345679.000100",
            "channel": "C-factory",
            "event_ts": "1712345679.000100"
        }
    })
    .to_string()
}

fn install_call(child_agent_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "connect_agent_to_slack",
            "arguments": {
                "agent_id": child_agent_id,
                "team_id": "T999",
                "channel_id": "C-install",
                "requested_by": "U999"
            }
        }
    })
}

fn content_text(value: &Value) -> &str {
    value["result"]["content"][0]["text"].as_str().unwrap()
}

fn content_json(value: &Value) -> Value {
    serde_json::from_str(content_text(value)).unwrap()
}
