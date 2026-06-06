use futures_util::StreamExt;
use litellm_rust::sdk::agents::{
    parse_sse, AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Lap, LapConfig, SendEventsParams, MANAGED_AGENTS_BETA,
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
        ..LapConfig::default()
    };
    Lap::new(config)
}

fn cursor_client(server: &MockServer) -> Lap {
    let config = LapConfig {
        cursor_api_key: Some("cursor-test".to_owned()),
        cursor_base_url: server.uri(),
        ..LapConfig::default()
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
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions"))
        .and(body_json(json!({
            "agent": "agent_123",
            "environment_id": "env_123",
            "title": "Quickstart session"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "sesn_123" })))
        .mount(&server)
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
        .mount(&server)
        .await;

    let client = client(&server);
    let environment = client
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
        .unwrap();
    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: "agent_123".to_owned(),
            environment_id: environment.id,
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: None,
            metadata: None,
            resources: None,
        })
        .await
        .unwrap();
    let sent = client
        .beta()
        .sessions()
        .events()
        .send(
            &session.id,
            SendEventsParams {
                events: vec![json!({
                    "type": "user.message",
                    "content": [{ "type": "text", "text": "Create fibonacci.txt" }]
                })],
            },
        )
        .await
        .unwrap();

    assert_eq!(session.id, "sesn_123");
    assert_eq!(sent.raw, json!({ "data": [] }));
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
    assert!(AgentRuntime::try_from("not-a-runtime").is_err());
}

#[tokio::test]
async fn cursor_provider_creates_runs_and_normalizes_stream_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agents"))
        .and(header("authorization", "Bearer cursor-test"))
        .and(body_json(json!({
            "prompt": { "text": "Create fibonacci.txt" },
            "model": { "id": "composer-2" },
            "repos": [{
                "url": "https://github.com/acme/app",
                "startingRef": "main"
            }],
            "name": "Quickstart session",
            "env": {
                "type": "cloud",
                "name": "quickstart-env"
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "agent": {
                "id": "bc-00000000-0000-0000-0000-000000000001",
                "name": "Quickstart session",
                "status": "ACTIVE",
                "latestRunId": "run-00000000-0000-0000-0000-000000000001"
            },
            "run": {
                "id": "run-00000000-0000-0000-0000-000000000001",
                "agentId": "bc-00000000-0000-0000-0000-000000000001",
                "status": "CREATING"
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(
            "/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs",
        ))
        .and(header("authorization", "Bearer cursor-test"))
        .and(body_json(json!({
            "prompt": { "text": "Add a troubleshooting note" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run": {
                "id": "run-00000000-0000-0000-0000-000000000002",
                "agentId": "bc-00000000-0000-0000-0000-000000000001",
                "status": "CREATING"
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/v1/agents/bc-00000000-0000-0000-0000-000000000001/runs/run-00000000-0000-0000-0000-000000000002/stream",
        ))
        .and(header("authorization", "Bearer cursor-test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "event: assistant\n\
             data: {\"text\":\"I'll update it.\"}\n\n\
             event: tool_call\n\
             data: {\"callId\":\"call-1\",\"name\":\"edit_file\",\"status\":\"running\",\"args\":{\"path\":\"README.md\"}}\n\n\
             event: result\n\
             data: {\"runId\":\"run-00000000-0000-0000-0000-000000000002\",\"status\":\"FINISHED\",\"text\":\"Done.\"}\n\n",
        ))
        .mount(&server)
        .await;

    let client = cursor_client(&server);
    let session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: "lap-agent-definition".to_owned(),
            environment_id: "quickstart-env".to_owned(),
            title: "Quickstart session".to_owned(),
            lap_agent_runtime: Some(AgentRuntime::Cursor),
            metadata: None,
            resources: Some(json!({
                "prompt": { "text": "Create fibonacci.txt" },
                "model": { "id": "composer-2" },
                "repos": [{
                    "url": "https://github.com/acme/app",
                    "startingRef": "main"
                }]
            })),
        })
        .await
        .unwrap();

    assert_eq!(session.id, "bc-00000000-0000-0000-0000-000000000001");

    client
        .beta()
        .sessions()
        .events()
        .send(
            &session.id,
            SendEventsParams {
                events: vec![json!({
                    "type": "user.message",
                    "content": [{ "type": "text", "text": "Add a troubleshooting note" }]
                })],
            },
        )
        .await
        .unwrap();

    let mut stream = client
        .beta()
        .sessions()
        .events()
        .stream(&session.id)
        .await
        .unwrap();
    let first = stream.next().await.unwrap().unwrap();
    let second = stream.next().await.unwrap().unwrap();
    let third = stream.next().await.unwrap().unwrap();

    assert_eq!(first.event_type, "agent.message");
    assert_eq!(first.data["content"][0]["text"], "I'll update it.");
    assert_eq!(second.event_type, "agent.tool_use");
    assert_eq!(second.data["name"], "edit_file");
    assert_eq!(second.data["id"], "call-1");
    assert_eq!(second.data["input"]["path"], "README.md");
    assert_eq!(third.event_type, "session.status_idle");
    assert_eq!(third.data["result"], "Done.");
}
