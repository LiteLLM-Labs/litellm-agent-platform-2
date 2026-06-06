#[path = "managed_agents_support/sdk.rs"]
mod sdk_support;

use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    parse_sse, AgentModel, AgentRuntime, CreateAgentParams, CreateSessionParams, Lap, LapConfig,
};
use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn creates_claude_managed_agent_with_anthropic_shape() {
    let server = MockServer::start().await;
    sdk_support::mount_claude_agent_create(&server).await;

    let agent = sdk_support::create_claude_agent(&server).await;

    assert_eq!(agent.id, "agent_123");
    assert_eq!(agent.version, Some(1));
}

#[tokio::test]
async fn creates_session_and_sends_events_with_runtime_ids() {
    let server = MockServer::start().await;
    sdk_support::mount_session_round_trip(&server).await;

    let (session, sent) = sdk_support::create_session_and_send_events(&server).await;

    assert_eq!(session.id, "sesn_123");
    assert_eq!(sent.raw, json!({ "data": [] }));
}

#[tokio::test]
async fn streams_session_events() {
    let server = MockServer::start().await;
    sdk_support::mount_session_stream(&server).await;

    let events = sdk_support::stream_mock_session_events(&server, "sesn_123").await;

    assert_eq!(events[0].event_type, "agent.message");
    assert_eq!(events[0].data["content"][0]["text"], "hello");
    assert_eq!(events[1].event_type, "session.status_idle");
}

#[tokio::test]
async fn creates_opencode_session_and_sends_message_parts() {
    let server = MockServer::start().await;
    sdk_support::mount_opencode_session_round_trip(&server).await;

    let (session, sent) = sdk_support::create_opencode_session_and_send(&server).await;

    assert_eq!(session.id, "sesn_open");
    assert_eq!(sent.raw["info"]["id"], "msg_123");
    assert_eq!(sent.raw["parts"][0]["text"], "done");
}

#[tokio::test]
async fn streams_opencode_global_events() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/event"))
        .and(header("authorization", "Basic b3BlbmNvZGU6cHc="))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "event: server.connected\n\
             data: {\"version\":\"1.0.0\"}\n\n\
             data: {\"type\":\"session.idle\",\"sessionID\":\"sesn_open\"}\n\n",
        ))
        .mount(&server)
        .await;

    let mut stream = sdk_support::opencode_client(&server)
        .beta()
        .sessions()
        .events()
        .stream("sesn_open")
        .await
        .unwrap();
    let first = stream.next().await.unwrap().unwrap();
    let second = stream.next().await.unwrap().unwrap();

    assert_eq!(first.event_type, "server.connected");
    assert_eq!(first.data["version"], "1.0.0");
    assert_eq!(second.event_type, "session.idle");
    assert_eq!(second.data["sessionID"], "sesn_open");
}

#[tokio::test]
async fn creates_opencode_session_with_bearer_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .and(header("authorization", "Bearer sk-master"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "sesn_bearer",
            "title": "Bearer session"
        })))
        .mount(&server)
        .await;

    let client = Lap::new(LapConfig {
        opencode_api_key: Some("sk-master".to_owned()),
        opencode_base_url: Some(server.uri()),
        ..LapConfig::default()
    });
    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams::opencode("Bearer session"))
        .await
        .unwrap();

    assert_eq!(session.id, "sesn_bearer");
}

#[tokio::test]
async fn rejects_opencode_agent_create_before_network() {
    let server = MockServer::start().await;
    let error = sdk_support::opencode_client(&server)
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::OpenCode,
            lap_provider_options: None,
            name: "Coding Assistant".to_owned(),
            model: AgentModel::from("anthropic/claude-sonnet-4-5"),
            system: "Write clean code.".to_owned(),
            description: None,
            tools: Vec::new(),
            mcp_servers: Vec::new(),
        })
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("agents.create is not supported for opencode"));
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
fn parses_sse_and_resolves_supported_runtimes() {
    let events = parse_sse(
        "event: agent.message\n\
         data: {\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n\n",
    )
    .unwrap();

    assert_eq!(events[0].event_type, "agent.message");
    assert_eq!(
        AgentRuntime::try_from("cursor").unwrap(),
        AgentRuntime::Cursor
    );
    assert_eq!(
        AgentRuntime::try_from("opencode").unwrap(),
        AgentRuntime::OpenCode
    );
    let catalog_ids: Vec<_> = AgentRuntime::catalog()
        .iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        catalog_ids,
        vec!["claude_managed_agents", "cursor", "opencode"]
    );
    assert!(AgentRuntime::try_from("not-a-runtime").is_err());
}

#[tokio::test]
async fn cursor_provider_stream_conforms_to_anthropic_reference_events() {
    let server = MockServer::start().await;
    sdk_support::mount_cursor_stream_conformance(&server).await;

    let (client, session) = sdk_support::create_cursor_session(&server).await;
    assert_eq!(session.id, sdk_support::CURSOR_AGENT_ID);

    let initial_events = sdk_support::stream_session_events(&client, &session.id).await;
    sdk_support::assert_initial_cursor_stream(&initial_events);

    sdk_support::register_cursor_session(&client, session.id);
    sdk_support::send_cursor_prompt(&client).await;

    let events =
        sdk_support::stream_session_events(&client, sdk_support::LAP_CURSOR_SESSION_ID).await;
    sdk_support::assert_cursor_events_match_reference(&events);
}
