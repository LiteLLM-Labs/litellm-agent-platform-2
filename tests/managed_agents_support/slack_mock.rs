use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

pub async fn mock_slack() -> MockServer {
    let server = MockServer::start().await;
    mount(
        &server,
        "/chat.postMessage",
        json!({
            "ok": true,
            "channel": "C123",
            "ts": "200.000001"
        }),
    )
    .await;
    mount(&server, "/chat.update", json!({ "ok": true })).await;
    mount(
        &server,
        "/conversations.open",
        json!({
            "ok": true,
            "channel": { "id": "D123" }
        }),
    )
    .await;
    mount(
        &server,
        "/users.lookupByEmail",
        json!({
            "ok": true,
            "user": { "id": "U-DM" }
        }),
    )
    .await;
    mount(&server, "/reactions.add", json!({ "ok": true })).await;
    mount(
        &server,
        "/oauth.v2.access",
        json!({
            "ok": true,
            "access_token": "xoxb-oauth-token",
            "bot_user_id": "B123",
            "team": { "name": "LiteLLM" }
        }),
    )
    .await;
    server
}

async fn mount(server: &MockServer, url_path: &'static str, body: Value) {
    Mock::given(method("POST"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}
