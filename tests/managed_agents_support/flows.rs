use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use super::{read_events_until_completed, request_json, request_raw, AppFixture};

mod claude_runtime;
mod cursor_runtime;
mod platform_mcps;
mod sessions;
mod slack;
mod slack_helpers;

pub use cursor_runtime::exercise_cursor_runtime_stream;
pub use platform_mcps::exercise_platform_mcps;
pub use sessions::exercise_sessions;
pub use slack::exercise_slack;

pub async fn assert_agent_runtime_catalog(fixture: &AppFixture) {
    let response = request_json(fixture.app.clone(), "GET", "/api/agent-runtimes", None).await;
    let runtimes = response["runtimes"].as_array().unwrap();
    let ids: Vec<_> = runtimes
        .iter()
        .map(|runtime| runtime["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["claude_managed_agents", "cursor", "opencode"]);
    assert!(!ids.contains(&"claude_agents"));
    assert_eq!(runtimes[2]["default_api_base"], "http://127.0.0.1:4096");
    assert_eq!(runtimes[0]["credential_provider_id"], "anthropic");
    assert_eq!(runtimes[1]["credential_provider_id"], "cursor");
    assert_eq!(runtimes[2]["credential_provider_id"], "opencode");
    let claude_tools: Vec<_> = runtimes[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        claude_tools,
        vec![
            "bash",
            "read",
            "write",
            "edit",
            "glob",
            "grep",
            "web_fetch",
            "web_search"
        ]
    );
    assert!(runtimes[2]["tools"].as_array().unwrap().is_empty());
}

pub async fn create_agent(fixture: &AppFixture) -> String {
    let created = request_json(
        fixture.app.clone(),
        "POST",
        "/api/agents",
        Some(json!({
            "name": "ops-agent",
            "owner_id": "user-1",
            "harness": "claude-code",
            "prompt": "watch deploys"
        })),
    )
    .await;
    created["id"].as_str().unwrap().to_owned()
}

pub async fn exercise_agent_lifecycle(fixture: &AppFixture, agent_id: &str) {
    let listed = request_json(
        fixture.app.clone(),
        "GET",
        "/api/agents?owner_id=user-1",
        None,
    )
    .await;
    assert_eq!(listed["agents"].as_array().unwrap().len(), 1);

    let paused = request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/agents/{agent_id}/pause"),
        None,
    )
    .await;
    assert_eq!(paused["status"], "paused");

    let resumed = request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/agents/{agent_id}/resume"),
        None,
    )
    .await;
    assert_eq!(resumed["status"], "active");
}

pub async fn exercise_memory(fixture: &AppFixture, agent_id: &str) {
    let memory = request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/agents/{agent_id}/memory"),
        Some(json!({"key": "deploys", "value": "watch prod", "always_on": true})),
    )
    .await;
    assert_eq!(memory["key"], "deploys");

    let memories = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/api/agents/{agent_id}/memory"),
        None,
    )
    .await;
    assert_eq!(memories["memories"].as_array().unwrap().len(), 1);

    request_json(
        fixture.app.clone(),
        "DELETE",
        &format!("/api/agents/{agent_id}/memory/deploys"),
        None,
    )
    .await;
}

pub async fn exercise_files(fixture: &AppFixture, agent_id: &str) {
    let file_path = format!("/api/agents/{agent_id}/files/notes.txt");
    request_raw(
        fixture.app.clone(),
        "PUT",
        &file_path,
        Some("hello".to_owned()),
        "text/plain",
        StatusCode::OK,
    )
    .await;

    let files = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/api/agents/{agent_id}/files"),
        None,
    )
    .await;
    assert_eq!(files["files"].as_array().unwrap().len(), 1);

    let file = request_raw(
        fixture.app.clone(),
        "GET",
        &file_path,
        None,
        "application/json",
        StatusCode::OK,
    )
    .await;
    assert_eq!(file, "hello");

    request_json(fixture.app.clone(), "DELETE", &file_path, None).await;
}

pub async fn exercise_runs(fixture: &AppFixture, agent_id: &str) {
    let run = request_json(
        fixture.app.clone(),
        "POST",
        &format!("/api/agents/{agent_id}/run"),
        Some(json!({"prompt": "say hello"})),
    )
    .await;
    let run_id = run["run_id"].as_str().unwrap().to_owned();
    assert_eq!(run["event_url"], "/event");
    assert!(run["logs_url"]
        .as_str()
        .unwrap()
        .contains(&format!("/api/agents/{agent_id}/runs/{run_id}/logs")));
    let events = read_events_until_completed(fixture.app.clone(), "/event", &run_id).await;
    assert!(events.contains("\"type\":\"message.part.delta\""));
    assert!(events.contains("\"delta\":\"hello \""));
    assert!(events.contains("\"delta\":\"from managed agent\\n\""));
    assert!(events.contains("\"type\":\"session.idle\""));

    let runs = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/api/agents/{agent_id}/runs"),
        None,
    )
    .await;
    assert_eq!(runs["runs"].as_array().unwrap().len(), 1);
    assert_eq!(runs["runs"][0]["status"], "completed");
    assert_eq!(runs["runs"][0]["sandbox_id"], "sbx_managed_test");

    let logs = request_raw(
        fixture.app.clone(),
        "GET",
        &format!("/api/agents/{agent_id}/runs/{run_id}/logs"),
        None,
        "application/json",
        StatusCode::OK,
    )
    .await;
    assert!(logs.contains("from managed agent"));
}

pub async fn exercise_skills(fixture: &AppFixture) {
    let skill = request_json(
        fixture.app.clone(),
        "POST",
        "/api/skills",
        Some(json!({"name": "triage", "content": "do triage", "owner_id": "user-1"})),
    )
    .await;
    let skill_id = skill["id"].as_str().unwrap();
    let skill = request_json(
        fixture.app.clone(),
        "PATCH",
        &format!("/api/skills/{skill_id}"),
        Some(json!({"description": "daily"})),
    )
    .await;
    assert_eq!(skill["description"], "daily");
}

pub async fn exercise_inbox(fixture: &AppFixture) {
    seed_inbox(&fixture.pool).await;
    let inbox = request_json(
        fixture.app.clone(),
        "GET",
        "/api/inbox?filter=attention",
        None,
    )
    .await;
    assert_eq!(inbox["items"].as_array().unwrap().len(), 2);

    request_json(
        fixture.app.clone(),
        "POST",
        "/api/approvals/appr_1/accept",
        Some(json!({"arguments": {"ok": true}})),
    )
    .await;
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/inbox/iss_1/resolve",
        Some(json!({"note": "done"})),
    )
    .await;
}

async fn seed_inbox(pool: &PgPool) {
    sqlx::query(
        r#"
        INSERT INTO "LiteLLM_ManagedAgentInboxItemsTable"
          (id, kind, title, status, created_at)
        VALUES
          ('appr_1', 'approval', 'approve deploy', 'pending', 1),
          ('iss_1', 'issue', 'deployment issue', 'open', 2)
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}
