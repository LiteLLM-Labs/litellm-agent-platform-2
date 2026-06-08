use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::super::{request_json, request_json_raw, AppFixture};

const VAULT_SECRET: &str = "real-browser-use-secret";

pub async fn exercise_opencode_runtime_omits_vault_plaintext(fixture: &AppFixture) {
    let opencode = save_opencode_credentials(fixture).await;
    let agent = create_opencode_agent_with_vault_key(fixture).await;
    save_vault_value(fixture).await;
    create_opencode_runtime_session(fixture, agent["id"].as_str().unwrap()).await;
    assert_provider_requests_do_not_contain_secret(&opencode).await;
}

async fn save_opencode_credentials(fixture: &AppFixture) -> MockServer {
    let opencode = MockServer::start().await;
    mount_opencode_runtime(&opencode).await;
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/providers/opencode",
        Some(json!({
            "api_key": "opencode-test",
            "api_base": opencode.uri()
        })),
    )
    .await;
    opencode
}

async fn create_opencode_agent_with_vault_key(fixture: &AppFixture) -> serde_json::Value {
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/agents",
        Some(json!({
            "name": "opencode-vault-agent",
            "owner_id": "user-1",
            "runtime": "opencode",
            "model": "anthropic/claude-sonnet-4-6",
            "system": "Use tools without asking for raw keys.",
            "vault_keys": ["BROWSER_USE_API_KEY"]
        })),
    )
    .await
}

async fn save_vault_value(fixture: &AppFixture) {
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/vault/user-1",
        Some(json!({
            "key": "BROWSER_USE_API_KEY",
            "value": VAULT_SECRET,
            "scope": "personal"
        })),
    )
    .await;
}

async fn create_opencode_runtime_session(fixture: &AppFixture, agent_id: &str) {
    let (status, body) = request_json_raw(
        fixture.app.clone(),
        "POST",
        "/session",
        Some(json!({
            "agent": agent_id,
            "agent_id": agent_id,
            "runtime": "opencode",
            "title": "OpenCode vault session",
            "prompt": "check vault",
            "environment": {}
        })),
    )
    .await;
    assert!(
        status.is_success(),
        "POST /session returned {status}: {body}"
    );
}

async fn assert_provider_requests_do_not_contain_secret(opencode: &MockServer) {
    let requests = opencode.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.url.path() == "/session"),
        "expected an OpenCode session create request"
    );
    for request in requests {
        let body = String::from_utf8_lossy(&request.body);
        assert!(
            !body.contains(VAULT_SECRET),
            "provider request leaked vault plaintext: {body}"
        );
    }
}

async fn mount_opencode_runtime(opencode: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/session"))
        .and(header(
            "authorization",
            "Basic b3BlbmNvZGU6b3BlbmNvZGUtdGVzdA==",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sesn_open",
            "title": "OpenCode vault session"
        })))
        .mount(opencode)
        .await;
    Mock::given(method("POST"))
        .and(path("/session/sesn_open/message"))
        .and(header(
            "authorization",
            "Basic b3BlbmNvZGU6b3BlbmNvZGUtdGVzdA==",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "info": { "id": "msg_123", "role": "assistant" },
            "parts": [{ "type": "text", "text": "done" }]
        })))
        .mount(opencode)
        .await;
}
