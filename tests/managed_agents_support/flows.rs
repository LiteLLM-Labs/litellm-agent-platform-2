use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::{read_events_until_completed, request_json, request_raw, AppFixture};

pub async fn create_agent(fixture: &AppFixture) -> String {
    let created = request_json(
        fixture.app.clone(),
        "POST",
        "/api/agents",
        Some(json!({
            "name": "ops-agent",
            "owner_id": "user-1",
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

pub async fn exercise_sessions(fixture: &AppFixture) {
    let session = request_json(
        fixture.app.clone(),
        "POST",
        "/session",
        Some(json!({"agent": "claude-code", "title": "chat proof"})),
    )
    .await;
    let session_id = session["id"].as_str().unwrap().to_owned();
    assert!(session_id.starts_with("ses_"));
    assert_eq!(session["title"], "chat proof");
    assert_eq!(session["harness"], "claude-code");

    let listed = request_json(fixture.app.clone(), "GET", "/session", None).await;
    assert!(listed
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == session_id));

    let initial_messages = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/session/{session_id}/message"),
        None,
    )
    .await;
    assert_eq!(initial_messages.as_array().unwrap().len(), 0);

    request_raw(
        fixture.app.clone(),
        "POST",
        &format!("/session/{session_id}/prompt_async"),
        Some(
            json!({
                "model": {"providerID": "litellm", "modelID": "claude-sonnet-4-6"},
                "parts": [{"type": "text", "text": "say hello"}]
            })
            .to_string(),
        ),
        "application/json",
        StatusCode::NO_CONTENT,
    )
    .await;

    let events = read_events_until_completed(fixture.app.clone(), "/event", &session_id).await;
    assert!(events.contains("\"type\":\"message.part.delta\""));
    assert!(events.contains("\"delta\":\"hello \""));
    assert!(events.contains("\"delta\":\"from managed agent\\n\""));

    let messages = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/session/{session_id}/message"),
        None,
    )
    .await;
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[0]["info"]["role"], "user");
    assert_eq!(messages[0]["parts"][0]["text"], "say hello");
    assert_eq!(messages[1]["info"]["role"], "assistant");
    assert_eq!(messages[1]["info"]["id"], session_id);
    assert_eq!(messages[1]["parts"][0]["id"], format!("{session_id}_text"));
    assert_eq!(
        messages[1]["parts"][0]["text"],
        "hello from managed agent\n"
    );

    let deleted = request_json(
        fixture.app.clone(),
        "DELETE",
        &format!("/session/{session_id}"),
        None,
    )
    .await;
    assert_eq!(deleted, true);
}

pub async fn exercise_cursor_runtime_stream(fixture: &AppFixture, agent_id: &str) {
    let cursor = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agents"))
        .and(header("authorization", "Bearer cursor-test"))
        .and(body_json(json!({
            "prompt": { "text": "watch deploys\n\nFix the failing tests" },
            "model": { "id": "composer-2" },
            "name": "ops-agent",
            "source": {
                "repository": "https://github.com/acme/app",
                "ref": "main"
            },
            "target": {
                "autoCreatePr": true,
                "branchName": "agent/cursor-proof"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "agent": {
                "id": "bc-11111111-1111-1111-1111-111111111111",
                "status": "ACTIVE",
                "latestRunId": "run-11111111-1111-1111-1111-111111111111"
            },
            "run": {
                "id": "run-11111111-1111-1111-1111-111111111111",
                "agentId": "bc-11111111-1111-1111-1111-111111111111",
                "status": "CREATING"
            }
        })))
        .mount(&cursor)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/v1/agents/bc-11111111-1111-1111-1111-111111111111/runs/run-11111111-1111-1111-1111-111111111111/stream",
        ))
        .and(header("authorization", "Bearer cursor-test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "event: status\n\
             data: {\"runId\":\"run-11111111-1111-1111-1111-111111111111\",\"status\":\"RUNNING\"}\n\n\
             event: assistant\n\
             data: {\"text\":\"gateway\"}\n\n\
             event: assistant\n\
             data: {\"text\":\" stream\"}\n\n\
             event: result\n\
             data: {\"runId\":\"run-11111111-1111-1111-1111-111111111111\",\"status\":\"FINISHED\"}\n\n",
        ))
        .mount(&cursor)
        .await;
    Mock::given(method("POST"))
        .and(path(
            "/v1/agents/bc-11111111-1111-1111-1111-111111111111/runs",
        ))
        .and(header("authorization", "Bearer cursor-test"))
        .and(body_json(json!({
            "prompt": { "text": "Follow up on the test failure" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run": {
                "id": "run-22222222-2222-2222-2222-222222222222",
                "agentId": "bc-11111111-1111-1111-1111-111111111111",
                "status": "CREATING"
            }
        })))
        .mount(&cursor)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/v1/agents/bc-11111111-1111-1111-1111-111111111111/runs/run-22222222-2222-2222-2222-222222222222/stream",
        ))
        .and(header("authorization", "Bearer cursor-test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "event: status\n\
             data: {\"runId\":\"run-22222222-2222-2222-2222-222222222222\",\"status\":\"RUNNING\"}\n\n\
             event: assistant\n\
             data: {\"text\":\"followup\"}\n\n\
             event: assistant\n\
             data: {\"text\":\" stream\"}\n\n\
             event: result\n\
             data: {\"runId\":\"run-22222222-2222-2222-2222-222222222222\",\"status\":\"FINISHED\"}\n\n",
        ))
        .mount(&cursor)
        .await;

    request_json(
        fixture.app.clone(),
        "PUT",
        "/api/agent-runtimes/cursor/credentials",
        Some(json!({
            "api_key": "cursor-test",
            "api_base": cursor.uri()
        })),
    )
    .await;
    let session = request_json(
        fixture.app.clone(),
        "POST",
        "/session",
        Some(json!({
            "runtime": "cursor",
            "agent_id": agent_id,
            "title": "cursor proof",
            "prompt": "Fix the failing tests",
            "environment": {
                "model": "composer-2",
                "repository": "https://github.com/acme/app",
                "ref": "main",
                "target_branch": "agent/cursor-proof",
                "auto_create_pr": true
            }
        })),
    )
    .await;
    let session_id = session["id"].as_str().unwrap();
    assert_eq!(session["runtime"], "cursor");
    assert_eq!(
        session["provider_session_id"],
        "bc-11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(
        session["provider_run_id"],
        "run-11111111-1111-1111-1111-111111111111"
    );

    let events = request_raw(
        fixture.app.clone(),
        "GET",
        &format!("/v1/sessions/{session_id}/events/stream?key=sk-local"),
        None,
        "application/json",
        StatusCode::OK,
    )
    .await;
    assert!(events.contains("\"type\":\"session.status_running\""));
    assert!(events.contains("\"type\":\"agent.message\""));
    assert!(events.contains("gateway stream"));
    assert!(events.contains("\"type\":\"session.status_idle\""));
    assert!(!events.contains("cursor."));

    request_raw(
        fixture.app.clone(),
        "POST",
        &format!("/session/{session_id}/prompt_async"),
        Some(
            json!({
                "parts": [{
                    "type": "text",
                    "text": "Follow up on the test failure"
                }]
            })
            .to_string(),
        ),
        "application/json",
        StatusCode::NO_CONTENT,
    )
    .await;
    let updated = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/session/{session_id}"),
        None,
    )
    .await;
    assert_eq!(
        updated["provider_run_id"],
        "run-22222222-2222-2222-2222-222222222222"
    );

    let events = request_raw(
        fixture.app.clone(),
        "GET",
        &format!("/v1/sessions/{session_id}/events/stream?key=sk-local"),
        None,
        "application/json",
        StatusCode::OK,
    )
    .await;
    assert!(events.contains("\"type\":\"agent.message\""));
    assert!(events.contains("followup stream"));
    assert!(!events.contains("cursor."));
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
