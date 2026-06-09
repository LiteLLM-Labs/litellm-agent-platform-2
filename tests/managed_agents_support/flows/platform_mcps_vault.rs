use serde_json::{json, Value};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

use crate::support::{request_json, AppFixture};

pub async fn assert_api_call_with_vault(fixture: &AppFixture, agent_id: &str) {
    save_pylon_key(fixture).await;
    attach_pylon_key(fixture, agent_id).await;
    let upstream = mock_pylon().await;
    assert_pylon_call_succeeds(fixture, agent_id, &upstream).await;
    assert_unattached_key_denied(fixture, agent_id, &upstream).await;
}

async fn save_pylon_key(fixture: &AppFixture) {
    request_json(
        fixture.app.clone(),
        "POST",
        "/api/vault/user-1",
        Some(json!({
            "key": "PYLON_API_KEY",
            "value": "secret-pylon-token"
        })),
    )
    .await;
}

async fn attach_pylon_key(fixture: &AppFixture, agent_id: &str) {
    request_json(
        fixture.app.clone(),
        "PATCH",
        &format!("/api/agents/{agent_id}"),
        Some(json!({ "vault_keys": ["PYLON_API_KEY"] })),
    )
    .await;
}

async fn mock_pylon() -> MockServer {
    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pylon/issues"))
        .and(header("Authorization", "Bearer secret-pylon-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "iss_123", "title": "Mock Pylon issue" }],
            "echo": "secret-pylon-token"
        })))
        .mount(&upstream)
        .await;
    upstream
}

async fn assert_pylon_call_succeeds(fixture: &AppFixture, agent_id: &str, upstream: &MockServer) {
    let proxied = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "api_call_with_vault",
                "arguments": {
                    "key": "PYLON_API_KEY",
                    "url": format!("{}/pylon/issues", upstream.uri()),
                    "auth": { "type": "bearer" }
                }
            }
        }),
    )
    .await;
    let content = content_text(&proxied);
    assert!(content.contains("\"status\": 200"));
    assert!(content.contains("Mock Pylon issue"));
    assert!(content.contains("[REDACTED_VAULT_CREDENTIAL]"));
    assert!(!content.contains("secret-pylon-token"));
}

async fn assert_unattached_key_denied(fixture: &AppFixture, agent_id: &str, upstream: &MockServer) {
    let denied = rpc(
        fixture,
        agent_id,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "api_call_with_vault",
                "arguments": {
                    "key": "UNATTACHED_API_KEY",
                    "url": format!("{}/pylon/issues", upstream.uri()),
                    "auth": { "type": "bearer" }
                }
            }
        }),
    )
    .await;
    let denied_content = content_text(&denied);
    assert!(denied_content.contains("vault key is not attached to this agent"));
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
