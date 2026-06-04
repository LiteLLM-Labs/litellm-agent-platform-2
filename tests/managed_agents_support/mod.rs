use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use futures_util::StreamExt;
use litellm_rust::{
    agents::config::E2bSandboxParams,
    db::managed_agents::pool as managed_agents_pool,
    http::routes::router,
    proxy::{
        config::{GatewayConfig, GeneralSettings},
        state::AppState,
    },
    sdk::{providers::transform::ProviderRegistry, router::Router as ModelRouter},
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::util::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

pub mod flows;

pub struct AppFixture {
    pub app: axum::Router,
    pool: PgPool,
    _e2b: MockServer,
}

impl AppFixture {
    pub async fn new() -> Option<Self> {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .ok()
            .filter(|url| !url.trim().is_empty())?;
        let pool = managed_agents_pool::connect(&database_url).await.unwrap();
        managed_agents_pool::migrate(&pool).await.unwrap();
        reset_tables(&pool).await;
        let e2b = mock_e2b().await;
        Some(Self {
            app: router(build_state(pool.clone(), e2b.uri())),
            pool,
            _e2b: e2b,
        })
    }
}

fn build_state(pool: PgPool, e2b_api_base: String) -> Arc<AppState> {
    let config = GatewayConfig {
        model_list: Vec::new(),
        mcp_servers: HashMap::new(),
        general_settings: GeneralSettings {
            master_key: Some("sk-local".to_owned()),
            database_url: Some("postgres://test".to_owned()),
            sandbox_choice: Some("e2b".to_owned()),
            e2b_sandbox_params: E2bSandboxParams {
                e2b_api_key: Some("e2b-test".to_owned()),
                e2b_template: "litellm-4gb".to_owned(),
                timeout_seconds: 1800,
                workspace_dir: "/home/user/workspace".to_owned(),
                e2b_api_base,
                envs: Default::default(),
            },
            ..Default::default()
        },
        agents: Vec::new(),
    };
    let http = AppState::build_http_client().unwrap();
    Arc::new(AppState::new(config, empty_router(), http, HashMap::new(), Some(pool)).unwrap())
}

fn empty_router() -> ModelRouter {
    ModelRouter::from_config(
        &GatewayConfig {
            model_list: Vec::new(),
            mcp_servers: HashMap::new(),
            general_settings: GeneralSettings::default(),
            agents: Vec::new(),
        },
        &ProviderRegistry::new(),
    )
    .unwrap()
}

pub async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> Value {
    let response = request(
        app,
        method,
        uri,
        body.map(|value| value.to_string()),
        "application/json",
    )
    .await;
    assert!(
        response.status().is_success(),
        "{} {} returned {}",
        method,
        uri,
        response.status()
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap_or_else(|_| json!({}))
}

pub async fn read_events_until_completed(app: axum::Router, event_url: &str) -> String {
    let response = request(
        app,
        "GET",
        &format!("{event_url}?key=sk-local"),
        None,
        "application/json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
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

pub async fn request_raw(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    content_type: &str,
    expected: StatusCode,
) -> String {
    let response = request(app, method, uri, body, content_type).await;
    assert_eq!(response.status(), expected);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

async fn mock_e2b() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/sandboxes"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "templateID": "litellm-4gb",
            "sandboxID": "sbx_managed_test",
            "clientID": "client_test",
            "envdVersion": "test",
            "alias": "base",
            "envdAccessToken": "envd-test",
            "trafficAccessToken": "traffic-test",
            "domain": server.uri()
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/process.Process/Start"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(connect_json_frames(&[
            br#"{"event":{"start":{"pid":1470}}}"#,
            br#"{"stdout":"eyJ0eXBlIjoidGV4dF9kZWx0YSIsInRleHQiOiJoZWxsbyAifQo="}"#,
            br#"{"stdout":"eyJ0eXBlIjoidGV4dF9kZWx0YSIsInRleHQiOiJmcm9tIG1hbmFnZWQgYWdlbnRcbiJ9Cg=="}"#,
            br#"{"event":{"end":{"exited":true,"status":"exit status 0"}}}"#,
        ])))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/sandboxes/sbx_managed_test"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    server
}

fn connect_json_frames(payloads: &[&[u8]]) -> Vec<u8> {
    let mut frames = Vec::new();
    for payload in payloads {
        frames.push(0);
        frames.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frames.extend_from_slice(payload);
    }
    frames
}

async fn request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<String>,
    content_type: &str,
) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, "Bearer sk-local")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body.unwrap_or_default()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn reset_tables(pool: &PgPool) {
    sqlx::query(
        r#"
        TRUNCATE
          "LiteLLM_ManagedAgentInboxItemsTable",
          "LiteLLM_ManagedAgentRunsTable",
          "LiteLLM_ManagedAgentFilesTable",
          "LiteLLM_ManagedAgentMemoriesTable",
          "LiteLLM_ManagedAgentsTable",
          "LiteLLM_ManagedAgentSessionsTable",
          "LiteLLM_ManagedAgentSkillsTable",
          "LiteLLM_SavedAgentsTable"
        CASCADE
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}
