use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use futures_util::StreamExt;
use litellm_rust::{
    agents::config::{AgentDefinition, OpenSandboxParams},
    http::routes::router,
    proxy::{
        config::{GatewayConfig, GeneralSettings},
        state::AppState,
    },
    sdk::{
        providers::{self, transform::ProviderRegistry},
        router::Router as ModelRouter,
    },
};
use serde_json::json;
use tower::util::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn starts_agent_and_streams_opensandbox_output() {
    let opensandbox = mock_opensandbox().await;
    let app = router(build_state(&test_config(opensandbox.uri())));

    let (event_url, run_id) = start_agent_run(&app).await;
    let body = read_events_until_completed(app, event_url).await;

    assert!(body.contains("\"type\":\"session.status\""));
    assert!(body.contains("\"type\":\"message.updated\""));
    assert!(body.contains("\"type\":\"message.part.updated\""));
    assert!(body.contains("\"type\":\"message.part.delta\""));
    assert!(body.contains("\"delta\":\"hello \""));
    assert!(body.contains("\"delta\":\"from sandbox\\n\""));
    assert!(body.contains("\"field\":\"text\""));
    // stderr frames are diagnostic noise and must not surface as output deltas.
    assert!(!body.contains("npm warn"));
    assert!(body.contains("\"type\":\"session.idle\""));
    assert!(body.contains(&run_id));
}

async fn mock_opensandbox() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/sandboxes"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "id": "sbx_test",
            "status": { "state": "Pending" },
            "createdAt": "2026-01-01T00:00:00Z",
            "entrypoint": ["tail", "-f", "/dev/null"]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sandboxes/sbx_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sbx_test",
            "status": { "state": "Running" }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/sandboxes/sbx_test/endpoints/44772"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "endpoint": server.uri(),
            "headers": { "X-EXECD-ACCESS-TOKEN": "execd-token" }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/command"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(sse_body(&[
            r#"{"type":"status","text":"running"}"#,
            r#"{"type":"stdout","text":"hello "}"#,
            r#"{"type":"stdout","text":"from sandbox\n"}"#,
            r#"{"type":"stderr","text":"npm warn deprecated"}"#,
            r#"{"type":"result","text":""}"#,
        ])))
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/sandboxes/sbx_test"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    server
}

fn sse_body(frames: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for frame in frames {
        out.extend_from_slice(frame.as_bytes());
        out.extend_from_slice(b"\n\n");
    }
    out
}

async fn start_agent_run(app: &axum::Router) -> (String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents/coder/run")
                .header(header::AUTHORIZATION, "Bearer sk-local")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "prompt": "say hello" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let event_url = body["event_url"].as_str().unwrap().to_owned();
    let run_id = body["run_id"].as_str().unwrap().to_owned();
    (event_url, run_id)
}

async fn read_events_until_completed(app: axum::Router, event_url: String) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("{event_url}?key=sk-local"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut body = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            body.push_str(std::str::from_utf8(&chunk).unwrap());
            if body.contains("\"type\":\"session.idle\"") {
                break;
            }
        }
        body
    })
    .await
    .unwrap()
}

fn test_config(api_base: String) -> GatewayConfig {
    GatewayConfig {
        model_list: Vec::new(),
        mcp_servers: HashMap::new(),
        general_settings: GeneralSettings {
            master_key: Some("sk-local".to_owned()),
            database_url: None,
            sandbox_choice: Some("opensandbox".to_owned()),
            opensandbox_sandbox_params: OpenSandboxParams {
                api_key: Some("osb-test".to_owned()),
                api_base,
                ..Default::default()
            },
            ..Default::default()
        },
        agents: vec![AgentDefinition {
            id: None,
            name: "Coder".to_owned(),
            description: Some("Coding agent".to_owned()),
            model: "claude-sonnet-4-6".to_owned(),
            harness: Some("opencode".to_owned()),
            system: "You are a coding agent.".to_owned(),
            mcp_servers: Vec::new(),
            tools: Vec::new(),
            skills: Vec::new(),
        }],
    }
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
