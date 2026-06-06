use reqwest::Method;
use serde_json::{json, Map, Value};

use super::{cursor_stream::normalize_cursor_stream, AdapterFuture, RuntimeAdapter};
use crate::sdk::agents::{
    client::{Lap, SessionContext},
    events::AgentEventStream,
    response_fields::{id, nested_id, nested_string_field},
    responses::response_json,
    types::{
        AgentModel, AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, ManagedSessionRef, SendEventsParams,
        SendEventsResponse, Session,
    },
};

pub(super) struct CursorRuntime;

impl RuntimeAdapter for CursorRuntime {
    fn configure_request(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        request.bearer_auth(api_key)
    }

    fn normalize_stream(&self, stream: AgentEventStream) -> AgentEventStream {
        normalize_cursor_stream(stream)
    }

    fn session_context(&self, session: ManagedSessionRef) -> SessionContext {
        SessionContext {
            runtime: session.lap_agent_runtime,
            provider_session_id: session.provider_session_id.clone(),
            agent_id: session.provider_agent_id.or(session.provider_session_id),
            run_id: session.provider_run_id,
        }
    }

    fn create_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateAgentParams,
    ) -> AdapterFuture<'a, ManagedAgent> {
        Box::pin(async move {
            let raw = client
                .post(
                    AgentRuntime::Cursor,
                    "/v1/agents",
                    &create_agent_body(params),
                )
                .await?;
            let agent_id = nested_id(&raw, "agent")?;
            if let Some(run_id) = run_id(&raw) {
                client.remember_cursor_run(&agent_id, &run_id)?;
            }
            Ok(ManagedAgent {
                id: agent_id,
                version: None,
                raw,
            })
        })
    }

    fn create_environment<'a>(
        &'a self,
        _client: &'a Lap,
        params: CreateEnvironmentParams,
    ) -> AdapterFuture<'a, Environment> {
        Box::pin(async move {
            let raw = json!({ "id": params.name });
            Ok(Environment { id: id(&raw)?, raw })
        })
    }

    fn create_session<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateSessionParams,
    ) -> AdapterFuture<'a, Session> {
        Box::pin(async move {
            if params.agent.trim().is_empty() {
                return Err(AgentSdkError::InvalidRequest(
                    "cursor sessions.create requires a non-empty Cursor agent id".to_owned(),
                ));
            }
            let raw = json!({ "id": params.agent });
            let session = Session { id: id(&raw)?, raw };
            let run_id = client.cursor_run_for_agent(&session.id)?;
            client.remember_session_context(
                &session.id,
                SessionContext::cursor(session.id.clone(), run_id),
            )?;
            Ok(session)
        })
    }

    fn send_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
        params: SendEventsParams,
    ) -> AdapterFuture<'a, SendEventsResponse> {
        Box::pin(async move {
            let agent_id = cursor_agent_id(client, session_id)?;
            let body = json!({ "prompt": prompt_from_events(&params.events)? });
            let raw = client
                .post(
                    AgentRuntime::Cursor,
                    &format!("/v1/agents/{agent_id}/runs"),
                    &body,
                )
                .await?;
            let run_id = nested_string_field(&raw, "run", "id")?;
            client.remember_session_context(
                session_id,
                SessionContext::cursor(agent_id, Some(run_id)),
            )?;
            Ok(SendEventsResponse { raw })
        })
    }

    fn stream_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
    ) -> AdapterFuture<'a, AgentEventStream> {
        Box::pin(async move {
            let context = client.context_for_session(session_id)?;
            let agent_id = agent_id_from_context(session_id, context.as_ref());
            let run_id = match context.and_then(|context| context.run_id) {
                Some(run_id) => run_id,
                None => latest_run_id(client, &agent_id).await?,
            };
            client
                .stream(
                    AgentRuntime::Cursor,
                    &format!("/v1/agents/{agent_id}/runs/{run_id}/stream"),
                )
                .await
        })
    }
}

fn run_id(raw: &Value) -> Option<String> {
    raw.get("run")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| {
            raw.get("agent")
                .and_then(|value| value.get("latestRunId"))
                .and_then(Value::as_str)
        })
        .or_else(|| raw.get("latestRunId").and_then(Value::as_str))
        .map(str::to_owned)
}

fn create_agent_body(params: CreateAgentParams) -> Value {
    let mut body = Map::new();
    body.insert("prompt".to_owned(), json!({ "text": params.system }));
    body.insert("name".to_owned(), Value::String(params.name));
    body.insert("model".to_owned(), model(params.model));
    if let Some(Value::Object(options)) = params.lap_provider_options {
        for (key, value) in options {
            body.insert(key, value);
        }
    }
    if !params.mcp_servers.is_empty() {
        body.insert(
            "mcpServers".to_owned(),
            Value::Array(params.mcp_servers.into_iter().map(mcp_server).collect()),
        );
    }
    Value::Object(body)
}

fn prompt_from_events(events: &[Value]) -> Result<Value, AgentSdkError> {
    let mut text = Vec::new();
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("user.message") {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
        }
    }
    if text.is_empty() {
        return Err(AgentSdkError::InvalidRequest(
            "cursor runtime requires at least one user.message text block".to_owned(),
        ));
    }
    Ok(json!({ "text": text.join("\n\n") }))
}

fn agent_id_from_context(session_id: &str, context: Option<&SessionContext>) -> String {
    context
        .and_then(|context| context.agent_id.clone())
        .or_else(|| context.and_then(|context| context.provider_session_id.clone()))
        .unwrap_or_else(|| session_id.to_owned())
}

fn model(model: AgentModel) -> Value {
    match model {
        AgentModel::Id(id) => json!({ "id": id }),
        AgentModel::Config(config) => {
            let mut model = Map::new();
            model.insert("id".to_owned(), Value::String(config.id));
            if let Some(speed) = config.speed {
                model.insert(
                    "params".to_owned(),
                    json!([{ "id": "speed", "value": speed }]),
                );
            }
            Value::Object(model)
        }
    }
}

fn mcp_server(server: Value) -> Value {
    let mut server = match server {
        Value::Object(server) => server,
        _ => Map::new(),
    };
    match server.get("type").and_then(Value::as_str) {
        Some("url") | None => {
            server.insert("type".to_owned(), Value::String("http".to_owned()));
        }
        _ => {}
    }
    Value::Object(server)
}

fn cursor_agent_id(client: &Lap, session_id: &str) -> Result<String, AgentSdkError> {
    Ok(agent_id_from_context(
        session_id,
        client.context_for_session(session_id)?.as_ref(),
    ))
}

async fn latest_run_id(client: &Lap, agent_id: &str) -> Result<String, AgentSdkError> {
    let response = client
        .request(
            AgentRuntime::Cursor,
            Method::GET,
            &format!("/v1/agents/{agent_id}"),
        )?
        .send()
        .await?;
    let raw = response_json(response).await?;
    run_id(&raw).ok_or(AgentSdkError::MissingField("latestRunId"))
}
