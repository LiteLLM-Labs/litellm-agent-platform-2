use futures_util::{future::BoxFuture, StreamExt};
use reqwest::{header, Method};
use serde_json::{json, Map, Value};

use super::super::transform::{
    ensure_success, nested_string_field, response_json, string_field, AgentRuntimeProvider,
    ProviderConfig, ProviderSendEventsResponse, ProviderSession, ProviderSessionContext,
};
use crate::sdk::agents::{
    events::{stream_events, AgentEvent, AgentEventStream},
    types::{
        AgentModel, AgentSdkError, CreateAgentParams, CreateEnvironmentParams, CreateSessionParams,
        Environment, ManagedAgent, SendEventsParams, SendEventsResponse, Session,
    },
};

#[derive(Debug, Clone)]
pub struct CursorProvider {
    config: ProviderConfig,
}

impl CursorProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, AgentSdkError> {
        let response = self.request(Method::POST, path).json(body).send().await?;
        response_json(response).await
    }

    async fn get(&self, path: &str) -> Result<Value, AgentSdkError> {
        let response = self.request(Method::GET, path).send().await?;
        response_json(response).await
    }

    async fn stream(&self, path: &str) -> Result<AgentEventStream, AgentSdkError> {
        let response = self
            .request(Method::GET, path)
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        let stream = stream_events(ensure_success(response).await?)
            .map(|event| event.map(normalize_cursor_event));
        Ok(Box::pin(stream))
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.config
            .http
            .request(method, self.config.url(path))
            .header(header::CONTENT_TYPE, "application/json")
            .bearer_auth(&self.config.api_key)
    }

    async fn latest_run_id(&self, agent_id: &str) -> Result<String, AgentSdkError> {
        let raw = self.get(&format!("/v1/agents/{agent_id}")).await?;
        string_field(&raw, "latestRunId")
    }
}

impl AgentRuntimeProvider for CursorProvider {
    fn create_agent<'a>(
        &'a self,
        params: CreateAgentParams,
    ) -> BoxFuture<'a, Result<ManagedAgent, AgentSdkError>> {
        Box::pin(async move {
            let raw = self
                .post("/v1/agents", &cursor_create_agent_body(params)?)
                .await?;
            Ok(ManagedAgent {
                id: nested_string_field(&raw, "agent", "id")?,
                version: None,
                raw,
            })
        })
    }

    fn create_environment<'a>(
        &'a self,
        params: CreateEnvironmentParams,
    ) -> BoxFuture<'a, Result<Environment, AgentSdkError>> {
        Box::pin(async move {
            let id = cursor_environment_id(&params);
            Ok(Environment {
                id: id.clone(),
                raw: json!({
                    "id": id,
                    "name": params.name,
                    "config": params.config,
                }),
            })
        })
    }

    fn create_session<'a>(
        &'a self,
        params: CreateSessionParams,
    ) -> BoxFuture<'a, Result<ProviderSession, AgentSdkError>> {
        Box::pin(async move {
            if is_cursor_agent_id(&params.agent) {
                let body = cursor_followup_body(params.resources)?;
                let raw = self
                    .post(&format!("/v1/agents/{}/runs", params.agent), &body)
                    .await?;
                let run_id = nested_string_field(&raw, "run", "id")?;
                return Ok(ProviderSession {
                    session: Session {
                        id: params.agent.clone(),
                        raw,
                    },
                    context: ProviderSessionContext {
                        runtime: self.config.runtime,
                        agent_id: Some(params.agent),
                        run_id: Some(run_id),
                    },
                });
            }

            let body = cursor_create_session_body(params)?;
            let raw = self.post("/v1/agents", &body).await?;
            let agent_id = nested_string_field(&raw, "agent", "id")?;
            let run_id = nested_string_field(&raw, "run", "id")?;
            Ok(ProviderSession {
                session: Session {
                    id: agent_id.clone(),
                    raw,
                },
                context: ProviderSessionContext {
                    runtime: self.config.runtime,
                    agent_id: Some(agent_id),
                    run_id: Some(run_id),
                },
            })
        })
    }

    fn send_events<'a>(
        &'a self,
        session_id: &'a str,
        params: SendEventsParams,
        context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<ProviderSendEventsResponse, AgentSdkError>> {
        Box::pin(async move {
            let agent_id = context
                .as_ref()
                .and_then(|context| context.agent_id.as_deref())
                .unwrap_or(session_id);
            let body = json!({ "prompt": prompt_from_events(&params.events)? });
            let raw = self
                .post(&format!("/v1/agents/{agent_id}/runs"), &body)
                .await?;
            let run_id = nested_string_field(&raw, "run", "id")?;
            Ok(ProviderSendEventsResponse {
                response: SendEventsResponse { raw },
                context: Some(ProviderSessionContext {
                    runtime: self.config.runtime,
                    agent_id: Some(agent_id.to_owned()),
                    run_id: Some(run_id),
                }),
            })
        })
    }

    fn stream_events<'a>(
        &'a self,
        session_id: &'a str,
        context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<AgentEventStream, AgentSdkError>> {
        Box::pin(async move {
            let agent_id = context
                .as_ref()
                .and_then(|context| context.agent_id.as_deref())
                .unwrap_or(session_id)
                .to_owned();
            let run_id = match context.and_then(|context| context.run_id) {
                Some(run_id) => run_id,
                None => self.latest_run_id(&agent_id).await?,
            };
            self.stream(&format!("/v1/agents/{agent_id}/runs/{run_id}/stream"))
                .await
        })
    }
}

fn cursor_create_agent_body(params: CreateAgentParams) -> Result<Value, AgentSdkError> {
    let mut body = Map::new();
    body.insert("prompt".to_owned(), json!({ "text": params.system }));
    body.insert("name".to_owned(), Value::String(params.name));
    body.insert("model".to_owned(), cursor_model(params.model));
    if !params.mcp_servers.is_empty() {
        body.insert("mcpServers".to_owned(), Value::Array(params.mcp_servers));
    }
    Ok(Value::Object(body))
}

fn cursor_create_session_body(params: CreateSessionParams) -> Result<Value, AgentSdkError> {
    let mut body = object_from_resources(params.resources)?;
    require_prompt(&body)?;
    body.entry("name".to_owned())
        .or_insert_with(|| Value::String(params.title));
    if !params.environment_id.trim().is_empty() && !body.contains_key("env") {
        body.insert(
            "env".to_owned(),
            json!({
                "type": "cloud",
                "name": params.environment_id,
            }),
        );
    }
    Ok(Value::Object(body))
}

fn cursor_followup_body(resources: Option<Value>) -> Result<Value, AgentSdkError> {
    let body = object_from_resources(resources)?;
    require_prompt(&body)?;
    let mut followup = Map::new();
    if let Some(prompt) = body.get("prompt") {
        followup.insert("prompt".to_owned(), prompt.clone());
    }
    if let Some(mcp_servers) = body.get("mcpServers") {
        followup.insert("mcpServers".to_owned(), mcp_servers.clone());
    }
    if let Some(mode) = body.get("mode") {
        followup.insert("mode".to_owned(), mode.clone());
    }
    Ok(Value::Object(followup))
}

fn object_from_resources(resources: Option<Value>) -> Result<Map<String, Value>, AgentSdkError> {
    match resources {
        Some(Value::Object(object)) => Ok(object),
        Some(_) => Err(AgentSdkError::InvalidRequest(
            "resources must be a JSON object for cursor runtime".to_owned(),
        )),
        None => Ok(Map::new()),
    }
}

fn require_prompt(body: &Map<String, Value>) -> Result<(), AgentSdkError> {
    if body
        .get("prompt")
        .and_then(|prompt| prompt.get("text"))
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
    {
        return Ok(());
    }
    Err(AgentSdkError::InvalidRequest(
        "cursor runtime requires resources.prompt.text".to_owned(),
    ))
}

fn cursor_environment_id(params: &CreateEnvironmentParams) -> String {
    params
        .config
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&params.name)
        .to_owned()
}

fn cursor_model(model: AgentModel) -> Value {
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

fn is_cursor_agent_id(value: &str) -> bool {
    value.starts_with("bc-")
}

fn prompt_from_events(events: &[Value]) -> Result<Value, AgentSdkError> {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("user.message") {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(value) = block.get("text").and_then(Value::as_str) {
                        text.push(value.to_owned());
                    }
                }
                Some("image") => images.push(cursor_image_block(block)?),
                _ => {}
            }
        }
    }
    if text.is_empty() {
        return Err(AgentSdkError::InvalidRequest(
            "cursor runtime requires at least one user.message text block".to_owned(),
        ));
    }
    let mut prompt = Map::new();
    prompt.insert("text".to_owned(), Value::String(text.join("\n\n")));
    if !images.is_empty() {
        prompt.insert("images".to_owned(), Value::Array(images));
    }
    Ok(Value::Object(prompt))
}

fn cursor_image_block(block: &Value) -> Result<Value, AgentSdkError> {
    if let Some(url) = block.get("url").and_then(Value::as_str) {
        return Ok(json!({ "url": url }));
    }
    if let Some(source) = block.get("source").and_then(Value::as_object) {
        if let (Some(data), Some(mime_type)) = (
            source.get("data").and_then(Value::as_str),
            source.get("mime_type").and_then(Value::as_str),
        ) {
            return Ok(json!({ "data": data, "mimeType": mime_type }));
        }
    }
    Err(AgentSdkError::InvalidRequest(
        "cursor image blocks require url or source.data/source.mime_type".to_owned(),
    ))
}

fn normalize_cursor_event(event: AgentEvent) -> AgentEvent {
    match event.event_type.as_str() {
        "assistant" => text_event("agent.message", event.data),
        "thinking" => text_event("agent.thinking", event.data),
        "tool_call" => tool_event(event.data),
        "status" => status_event(event.data),
        "result" => result_event(event.data),
        "done" => simple_event("session.status_idle", event.data),
        "error" => simple_event("session.error", event.data),
        "heartbeat" => simple_event("session.heartbeat", event.data),
        _ => simple_event(&format!("cursor.{}", event.event_type), event.data),
    }
}

fn text_event(event_type: &str, mut data: Map<String, Value>) -> AgentEvent {
    if let Some(text) = data.get("text").cloned() {
        data.insert(
            "content".to_owned(),
            json!([{ "type": "text", "text": text }]),
        );
    }
    simple_event(event_type, data)
}

fn tool_event(mut data: Map<String, Value>) -> AgentEvent {
    if let Some(call_id) = data.remove("callId") {
        data.insert("id".to_owned(), call_id);
    }
    if let Some(args) = data.remove("args") {
        data.insert("input".to_owned(), args);
    }
    simple_event("agent.tool_use", data)
}

fn status_event(data: Map<String, Value>) -> AgentEvent {
    match data.get("status").and_then(Value::as_str) {
        Some("FINISHED") => simple_event("session.status_idle", data),
        Some("ERROR") | Some("CANCELLED") | Some("EXPIRED") => simple_event("session.error", data),
        Some("RUNNING") => simple_event("session.status_running", data),
        Some("CREATING") => simple_event("session.status_creating", data),
        _ => simple_event("session.status", data),
    }
}

fn result_event(mut data: Map<String, Value>) -> AgentEvent {
    if let Some(text) = data.remove("text") {
        data.insert("result".to_owned(), text);
    }
    status_event(data)
}

fn simple_event(event_type: &str, data: Map<String, Value>) -> AgentEvent {
    AgentEvent {
        event_type: event_type.to_owned(),
        data,
    }
}
