use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::super::{request_json, AppFixture};

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
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sesn_111111111111111111111111/events"))
        .and(header("x-api-key", "anthropic-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
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
