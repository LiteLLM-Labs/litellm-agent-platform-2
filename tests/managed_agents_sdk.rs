use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    parse_sse, AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Environment, Lap, LapConfig, SendEventsParams, SendEventsResponse,
    Session, MANAGED_AGENTS_BETA,
};
use serde_json::json;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn client(server: &MockServer) -> Lap {
    let config = LapConfig {
        anthropic_api_key: Some("sk-ant-test".to_owned()),
        anthropic_base_url: server.uri(),
    };
    Lap::new(config)
}

#[tokio::test]
async fn creates_claude_managed_agent_with_anthropic_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agents"))
        .and(header("x-api-key", "sk-ant-test"))
        .and(header("anthropic-beta", MANAGED_AGENTS_BETA))
        .and(body_json(json!({
            "name": "Coding Assistant",
            "model": "claude-opus-4-8",
            "system": "Write clean code.",
            "tools": [{ "type": "agent_toolset_20260401" }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "agent_123",
            "version": 1
        })))
        .mount(&server)
        .await;

    let agent = client(&server)
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            name: "Coding Assistant".to_owned(),
            model: AgentModel::from("claude-opus-4-8"),
            system: "Write clean code.".to_owned(),
            description: None,
            tools: vec![json!({ "type": "agent_toolset_20260401" })],
            mcp_servers: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(agent.id, "agent_123");
    assert_eq!(agent.version, Some(1));
}

#[tokio::test]
async fn creates_session_and_sends_events_with_runtime_ids() {
    let server = MockServer::start().await;
    mount_session_event_mocks(&server).await;

    let client = client(&server);
    let environment = create_environment(&client).await;
    let session = create_session(&client, environment.id).await;
    let sent = send_user_event(&client, &session.id).await;

    assert_eq!(session.id, "sesn_123");
    assert_eq!(sent.raw, json!({ "data": [] }));
}

async fn mount_session_event_mocks(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/environments"))
        .and(body_json(json!({
            "name": "quickstart-env",
            "config": {
                "type": "cloud",
                "networking": { "type": "unrestricted" }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "env_123" })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions"))
        .and(body_json(json!({
            "agent": "agent_123",
            "environment_id": "env_123",
            "title": "Quickstart session"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "sesn_123" })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sesn_123/events"))
        .and(body_json(json!({
            "events": [{
                "type": "user.message",
                "content": [{ "type": "text", "text": "Create fibonacci.txt" }]
            }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .mount(server)
        .await;
}

async fn create_environment(client: &Lap) -> Environment {
    client
        .beta()
        .environments()
        .create(CreateEnvironmentParams {
            lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
            name: "quickstart-env".to_owned(),
            config: json!({ "type": "cloud", "networking": { "type": "unrestricted" } }),
            description: None,
            scope: None,
        })
        .await
        .unwrap()
}

async fn create_session(client: &Lap, environment_id: String) -> Session {
    client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: "agent_123".to_owned(),
            environment_id,
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: None,
            metadata: None,
            resources: None,
        })
        .await
        .unwrap()
}

async fn send_user_event(client: &Lap, session_id: &str) -> SendEventsResponse {
    client
        .beta()
        .sessions()
        .events()
        .send(
            session_id,
            SendEventsParams {
                events: vec![json!({
                    "type": "user.message",
                    "content": [{ "type": "text", "text": "Create fibonacci.txt" }]
                })],
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn streams_session_events() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/sessions/sesn_123/events/stream"))
        .and(header("anthropic-beta", MANAGED_AGENTS_BETA))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "event: agent.message\n\
             data: {\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n\n\
             data: {\"type\":\"session.status_idle\"}\n\n",
        ))
        .mount(&server)
        .await;

    let mut stream = client(&server)
        .beta()
        .sessions()
        .events()
        .stream("sesn_123")
        .await
        .unwrap();
    let first = stream.next().await.unwrap().unwrap();
    let second = stream.next().await.unwrap().unwrap();

    assert_eq!(first.event_type, "agent.message");
    assert_eq!(first.data["content"][0]["text"], "hello");
    assert_eq!(second.event_type, "session.status_idle");
}

#[test]
fn parses_sse_and_rejects_unknown_runtime() {
    let events = parse_sse(
        "event: agent.message\n\
         data: {\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}\n\n",
    )
    .unwrap();

    assert_eq!(events[0].event_type, "agent.message");
    assert!(AgentRuntime::try_from("cursor").is_err());
}
