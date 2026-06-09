use serde_json::{json, Value};

use crate::support::{request_json, AppFixture};

pub async fn assert_human_approval(fixture: &AppFixture, agent_id: &str) {
    assert_accepts_approval(fixture, agent_id).await;
    assert_rejects_with_feedback(fixture, agent_id).await;
}

async fn assert_accepts_approval(fixture: &AppFixture, agent_id: &str) {
    let pending = spawn_approval_call(fixture, agent_id, 7, "approve deploy", "prod");
    let approval_id = wait_for_approval_item(fixture, "approve deploy").await;
    request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/approvals/{approval_id}/accept"),
        Some(json!({"arguments": {"environment": "staging"}})),
    )
    .await;
    let result = pending.await.unwrap();
    let content = content_text(&result);
    assert!(content.contains(&approval_id));
    assert!(content.contains("\"status\": \"accepted\""));
    assert!(content.contains("\"environment\": \"staging\""));
}

async fn assert_rejects_with_feedback(fixture: &AppFixture, agent_id: &str) {
    let pending = spawn_approval_call(fixture, agent_id, 8, "reject deploy", "prod");
    let approval_id = wait_for_approval_item(fixture, "reject deploy").await;
    request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/approvals/{approval_id}/reject"),
        Some(json!({"feedback": "Need a canary plan."})),
    )
    .await;
    let result = pending.await.unwrap();
    let content = content_text(&result);
    assert!(content.contains(&approval_id));
    assert!(content.contains("\"status\": \"rejected\""));
    assert!(content.contains("Need a canary plan."));
}

fn spawn_approval_call(
    fixture: &AppFixture,
    agent_id: &str,
    id: i32,
    title: &str,
    environment: &str,
) -> tokio::task::JoinHandle<Value> {
    let app = fixture.app.clone();
    let agent_id = agent_id.to_owned();
    let title = title.to_owned();
    let environment = environment.to_owned();
    tokio::spawn(async move {
        request_json(
            app,
            "POST",
            &format!("/mcp/platform/{agent_id}"),
            approval_call(id, &title, &environment),
        )
        .await
    })
}

fn approval_call(id: i32, title: &str, environment: &str) -> Option<Value> {
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "request_human_approval",
            "arguments": {
                "title": title,
                "body": "Deploy production after smoke tests pass.",
                "arguments": { "environment": environment }
            }
        }
    }))
}

async fn wait_for_approval_item(fixture: &AppFixture, title: &str) -> String {
    for _ in 0..20 {
        let inbox = request_json(
            fixture.app.clone(),
            "GET",
            "/api/inbox?filter=attention",
            None,
        )
        .await;
        let found = inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| is_target_approval(item, title));
        if let Some(item) = found {
            let args = serde_json::from_str::<Value>(item["args_json"].as_str().unwrap()).unwrap();
            assert_eq!(args["environment"], json!("prod"));
            return item["id"].as_str().unwrap().to_owned();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("approval item did not land in inbox");
}

fn is_target_approval(item: &Value, title: &str) -> bool {
    item["kind"] == "approval" && item["status"] == "pending" && item["title"] == title
}

fn content_text(value: &Value) -> &str {
    value["result"]["content"][0]["text"].as_str().unwrap()
}
