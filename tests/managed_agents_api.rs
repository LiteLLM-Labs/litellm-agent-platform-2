#[path = "managed_agents_support/mod.rs"]
mod support;

use serde_json::json;
use support::{flows, request_json, AppFixture};

static DB_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn managed_agent_endpoints_round_trip_against_postgres() {
    let _guard = DB_TEST_LOCK.lock().await;
    let Some(fixture) = AppFixture::new().await else {
        eprintln!("skipping managed agent integration test: TEST_DATABASE_URL is not set");
        return;
    };

    flows::assert_agent_runtime_catalog(&fixture).await;
    let agent_id = flows::create_agent(&fixture).await;
    flows::exercise_agent_lifecycle(&fixture, &agent_id).await;
    flows::exercise_memory(&fixture, &agent_id).await;
    flows::exercise_files(&fixture, &agent_id).await;
    flows::exercise_runs(&fixture, &agent_id).await;
    flows::exercise_sessions(&fixture).await;
    flows::exercise_cursor_runtime_stream(&fixture, &agent_id).await;
    flows::exercise_skills(&fixture).await;
    flows::exercise_inbox(&fixture).await;

    request_json(
        fixture.app.clone(),
        "DELETE",
        &format!("/api/agents/{agent_id}"),
        None,
    )
    .await;
}

#[tokio::test]
async fn rejects_invalid_file_base64_against_postgres() {
    let _guard = DB_TEST_LOCK.lock().await;
    let Some(fixture) = AppFixture::new().await else {
        eprintln!("skipping managed agent integration test: TEST_DATABASE_URL is not set");
        return;
    };

    let agent_id = flows::create_agent(&fixture).await;
    support::request_raw(
        fixture.app.clone(),
        "PUT",
        &format!("/api/agents/{agent_id}/files/bad.xlsx"),
        Some(json!({"content_base64": "not base64 !!!"}).to_string()),
        "application/json",
        axum::http::StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
async fn runtime_agent_create_keeps_legacy_harness_against_postgres() {
    let _guard = DB_TEST_LOCK.lock().await;
    let Some(fixture) = AppFixture::new().await else {
        eprintln!("skipping managed agent integration test: TEST_DATABASE_URL is not set");
        return;
    };

    let created = request_json(
        fixture.app.clone(),
        "POST",
        "/api/agents",
        Some(json!({
            "name": "runtime-agent",
            "owner_id": "user-1",
            "runtime": "claude_managed_agents",
            "harness": "claude_managed_agents",
            "config": { "runtime": "claude_managed_agents" }
        })),
    )
    .await;
    assert_eq!(created["harness"], "claude-code");
    assert!(created["tools"].is_null());
    assert_eq!(created["config"]["runtime"], "claude_managed_agents");

    let explicit_empty_tools = request_json(
        fixture.app.clone(),
        "POST",
        "/api/agents",
        Some(json!({
            "name": "empty-tools-agent",
            "owner_id": "user-1",
            "runtime": "claude_managed_agents",
            "tools": []
        })),
    )
    .await;
    assert_eq!(explicit_empty_tools["tools"], json!([]));
    assert_eq!(explicit_empty_tools["config"]["tools"], json!([]));
}
