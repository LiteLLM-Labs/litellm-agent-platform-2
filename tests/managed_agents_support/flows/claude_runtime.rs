use serde_json::json;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::super::{read_events_until_completed, request_json, AppFixture};

pub async fn save_anthropic_credentials(fixture: &AppFixture) -> MockServer {
    let anthropic = MockServer::start().await;
    mount_claude_runtime(&anthropic).await;
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/providers/anthropic",
        Some(json!({
            "api_key": "anthropic-test",
            "api_base": anthropic.uri()
        })),
    )
    .await;
    anthropic
}

pub async fn exercise_claude_runtime_session_storage(fixture: &AppFixture, agent_id: &str) {
    let anthropic = save_anthropic_credentials(fixture).await;
    let session = create_claude_runtime_session(fixture, agent_id).await;
    let session_id = session["id"].as_str().unwrap();
    assert_claude_session_response(&session);
    assert_claude_session_stored(fixture, session_id).await;
    assert_claude_runtime_events_stored(fixture, session_id).await;
    anthropic.verify().await;
}

async fn create_claude_runtime_session(fixture: &AppFixture, agent_id: &str) -> serde_json::Value {
    request_json(
        fixture.app.clone(),
        "POST",
        "/session",
        Some(json!({
            "agent": agent_id,
            "agent_id": agent_id,
            "runtime": "claude_managed_agents",
            "title": "Claude runtime storage",
            "prompt": "say hello"
        })),
    )
    .await
}

fn assert_claude_session_response(session: &serde_json::Value) {
    let session_id = session["id"].as_str().unwrap();
    assert!(session_id.starts_with("ses_"));
    assert_eq!(session["runtime"], "claude_managed_agents");
    assert_eq!(
        session["provider_session_id"],
        "sesn_111111111111111111111111"
    );
}

async fn assert_claude_session_stored(fixture: &AppFixture, session_id: &str) {
    let stored = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/session/{session_id}"),
        None,
    )
    .await;
    assert_eq!(
        stored["provider_session_id"],
        "sesn_111111111111111111111111"
    );
}

async fn assert_claude_runtime_events_stored(fixture: &AppFixture, session_id: &str) {
    let events = read_events_until_completed(
        fixture.app.clone(),
        &format!("/v1/sessions/{session_id}/events/stream"),
        session_id,
    )
    .await;
    assert!(events.contains("hello from managed agent"));

    let replay = request_json(
        fixture.app.clone(),
        "GET",
        &format!("/v1/sessions/{session_id}/events"),
        None,
    )
    .await;
    assert_claude_replay_events(&replay);
}

fn assert_claude_replay_events(replay: &serde_json::Value) {
    let replay_events = replay["data"].as_array().unwrap();
    assert!(replay_events
        .iter()
        .any(|event| event["type"] == "agent.message"));
    assert!(replay_events
        .iter()
        .any(|event| event["type"] == "session.status_idle"));
}

async fn mount_claude_runtime(anthropic: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/agents"))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ag_111111111111111111111111"
        })))
        .mount(anthropic)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/environments"))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "env_111111111111111111111111"
        })))
        .mount(anthropic)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions"))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sesn_111111111111111111111111"
        })))
        .mount(anthropic)
        .await;
    mount_claude_session_events(anthropic).await;
}

async fn mount_claude_session_events(anthropic: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sesn_111111111111111111111111/events"))
        .and(header("x-api-key", "anthropic-test"))
        .and(body_json(json!({
            "events": [{
                "type": "user.message",
                "content": [{ "type": "text", "text": "say hello" }]
            }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1..)
        .with_priority(1)
        .mount(anthropic)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sesn_111111111111111111111111/events"))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .with_priority(2)
        .mount(anthropic)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/v1/sessions/sesn_111111111111111111111111/events/stream",
        ))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "data: {\"type\":\"agent.message\",\"content\":[{\"type\":\"text\",\"text\":\"hello from managed agent\\n\"}]}\n\n\
             data: {\"type\":\"session.status_idle\"}\n\n",
        ))
        .mount(anthropic)
        .await;
}
