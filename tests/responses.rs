use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use litellm_rust::{
    http::routes::router,
    proxy::{
        config::{GatewayConfig, GeneralSettings, LiteLlmParams, ModelEntry},
        state::AppState,
    },
    sdk::{
        providers::{self, transform::ProviderRegistry},
        router::Router as ModelRouter,
    },
};
use serde_json::{json, Value};
use tower::util::ServiceExt;
use wiremock::{
    matchers::{header as header_match, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn test_config(api_base: String) -> GatewayConfig {
    GatewayConfig {
        model_list: vec![ModelEntry {
            model_name: "codex".to_owned(),
            litellm_params: LiteLlmParams {
                model: "anthropic/claude-sonnet-4-5".to_owned(),
                api_key: Some("sk-ant-test".to_owned()),
                api_base: Some(api_base),
                litellm_credential_name: None,
                extra: Default::default(),
            },
        }],
        mcp_servers: HashMap::new(),
        general_settings: GeneralSettings {
            master_key: Some("sk-local".to_owned()),
            ..Default::default()
        },
        agents: Vec::new(),
    }
}

#[tokio::test]
async fn forwards_responses_request_as_anthropic_messages() {
    let upstream = MockServer::start().await;
    mount_json_upstream(&upstream).await;
    let response = request_responses(&upstream, responses_json_body()).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = read_json(response).await;
    assert_eq!(response_body["object"], "response");
    assert_eq!(
        response_body.pointer("/output/0/content/0/text"),
        Some(&json!("pong"))
    );

    let requests = upstream.received_requests().await.unwrap();
    let upstream_body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(upstream_body["model"], "claude-sonnet-4-5");
    assert_eq!(upstream_body["max_tokens"], 64);
    assert_eq!(upstream_body["system"], "You are concise.");
    assert_eq!(
        upstream_body.pointer("/messages/0/role"),
        Some(&json!("user"))
    );
    assert_eq!(
        upstream_body.pointer("/messages/0/content/0/text"),
        Some(&json!("ping"))
    );
    assert_eq!(
        upstream_body.pointer("/tools/0/name"),
        Some(&json!("exec_command"))
    );
}

#[tokio::test]
async fn converts_anthropic_stream_to_responses_sse() {
    let upstream = MockServer::start().await;
    mount_stream_upstream(&upstream).await;
    let response = request_responses(
        &upstream,
        json!({ "model": "codex", "input": "ping", "stream": true }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    let body = read_text(response).await;
    assert!(body.contains("event: response.output_text.delta"));
    assert!(body.contains("\"delta\":\"pong\""));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("data: [DONE]"));
}

async fn mount_json_upstream(upstream: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header_match("x-api-key", "sk-ant-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "content": [{"type": "text", "text": "pong"}],
            "usage": {"input_tokens": 2, "output_tokens": 1}
        })))
        .mount(upstream)
        .await;
}

async fn mount_stream_upstream(upstream: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    concat!(
                        "event: message_start\n",
                        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
                        "event: content_block_start\n",
                        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                        "event: content_block_delta\n",
                        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong\"}}\n\n",
                        "event: content_block_stop\n",
                        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                        "event: message_delta\n",
                        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                        "event: message_stop\n",
                        "data: {\"type\":\"message_stop\"}\n\n"
                    ),
                ),
        )
        .mount(upstream)
        .await;
}

async fn request_responses(upstream: &MockServer, body: Value) -> axum::response::Response {
    let config = test_config(upstream.uri());
    let app = router(build_state(&config));
    app.oneshot(responses_request(body)).await.unwrap()
}

fn responses_request(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-local")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn responses_json_body() -> Value {
    json!({
        "model": "codex",
        "instructions": "You are concise.",
        "max_output_tokens": 64,
        "input": [user_text_message("ping")],
        "tools": [function_tool(), namespace_tool()],
        "stream": false
    })
}

fn user_text_message(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "user",
        "content": [{ "type": "input_text", "text": text }]
    })
}

fn function_tool() -> Value {
    json!({
        "type": "function",
        "name": "exec_command",
        "description": "Run a command",
        "parameters": {
            "type": "object",
            "properties": { "cmd": { "type": "string" } },
            "required": ["cmd"]
        }
    })
}

fn namespace_tool() -> Value {
    json!({
        "type": "namespace",
        "name": "multi_agent_v1",
        "tools": [{
            "type": "function",
            "name": "spawn_agent",
            "parameters": { "type": "object", "properties": {} }
        }]
    })
}

async fn read_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn read_text(response: axum::response::Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn build_router(config: &GatewayConfig) -> ModelRouter {
    let mut providers = ProviderRegistry::new();
    providers::register_all(&mut providers);
    ModelRouter::from_config(config, &providers).unwrap()
}

fn build_state(config: &GatewayConfig) -> Arc<AppState> {
    let http = AppState::build_http_client().unwrap();
    Arc::new(
        AppState::new(
            config.clone(),
            build_router(config),
            http,
            HashMap::new(),
            None,
        )
        .unwrap(),
    )
}
