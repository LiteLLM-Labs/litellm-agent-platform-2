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
        Environment, ImageSource, ManagedAgent, ManagedAgentMcpServer, ManagedAgentMcpServerType,
        SendEventsParams, SendEventsResponse, Session, UserContentBlock, UserEvent,
    },
};

#[derive(Debug, Clone)]
pub struct CursorProvider {
    config: ProviderConfig,
}

pub const CREATE_AGENT_PARAMS: &[&str] = &["name", "model", "system", "mcp_servers"];
pub const CREATE_ENVIRONMENT_PARAMS: &[&str] = &[];
pub const CREATE_SESSION_PARAMS: &[&str] = &["agent"];
pub const SEND_EVENTS_PARAMS: &[&str] = &["events"];

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
    fn supported_managed_agents_create_agent_params(&self) -> &'static [&'static str] {
        CREATE_AGENT_PARAMS
    }

    fn transform_managed_agents_create_agent_params(
        &self,
        params: CreateAgentParams,
    ) -> Result<Value, AgentSdkError> {
        cursor_create_agent_body(params)
    }

    fn create_agent<'a>(
        &'a self,
        params: CreateAgentParams,
    ) -> BoxFuture<'a, Result<ManagedAgent, AgentSdkError>> {
        Box::pin(async move {
            let raw = self
                .post(
                    "/v1/agents",
                    &self.transform_managed_agents_create_agent_params(params)?,
                )
                .await?;
            Ok(ManagedAgent {
                id: nested_string_field(&raw, "agent", "id")?,
                version: None,
                raw,
            })
        })
    }

    fn supported_managed_agents_create_environment_params(&self) -> &'static [&'static str] {
        CREATE_ENVIRONMENT_PARAMS
    }

    fn transform_managed_agents_create_environment_params(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<Value, AgentSdkError> {
        Ok(json!({ "id": params.name }))
    }

    fn create_environment<'a>(
        &'a self,
        params: CreateEnvironmentParams,
    ) -> BoxFuture<'a, Result<Environment, AgentSdkError>> {
        Box::pin(async move {
            let raw = self.transform_managed_agents_create_environment_params(params)?;
            let id = string_field(&raw, "id")?;
            Ok(Environment { id, raw })
        })
    }

    fn supported_managed_agents_create_session_params(&self) -> &'static [&'static str] {
        CREATE_SESSION_PARAMS
    }

    fn transform_managed_agents_create_session_params(
        &self,
        params: CreateSessionParams,
    ) -> Result<Value, AgentSdkError> {
        let agent_id = params.agent.id().to_owned();
        if !is_cursor_agent_id(&agent_id) {
            return Err(AgentSdkError::InvalidRequest(
                "cursor sessions.create requires a Cursor runtime agent id returned by agents.create"
                    .to_owned(),
            ));
        }
        Ok(json!({ "id": agent_id }))
    }

    fn create_session<'a>(
        &'a self,
        params: CreateSessionParams,
    ) -> BoxFuture<'a, Result<ProviderSession, AgentSdkError>> {
        Box::pin(async move {
            let raw = self.transform_managed_agents_create_session_params(params)?;
            let agent_id = string_field(&raw, "id")?;
            Ok(ProviderSession {
                session: Session {
                    id: agent_id.clone(),
                    raw,
                },
                context: ProviderSessionContext {
                    runtime: self.config.runtime,
                    agent_id: Some(agent_id),
                    run_id: None,
                },
            })
        })
    }

    fn supported_managed_agents_send_events_params(&self) -> &'static [&'static str] {
        SEND_EVENTS_PARAMS
    }

    fn transform_managed_agents_send_events_params(
        &self,
        params: SendEventsParams,
    ) -> Result<Value, AgentSdkError> {
        Ok(json!({ "prompt": prompt_from_events(&params.events)? }))
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
            let body = self.transform_managed_agents_send_events_params(params)?;
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
        body.insert(
            "mcpServers".to_owned(),
            Value::Array(cursor_mcp_servers(params.mcp_servers)),
        );
    }
    Ok(Value::Object(body))
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

fn cursor_mcp_servers(servers: Vec<ManagedAgentMcpServer>) -> Vec<Value> {
    servers
        .into_iter()
        .map(|server| {
            json!({
                "name": server.name,
                "type": cursor_mcp_server_type(server.server_type),
                "url": server.url,
            })
        })
        .collect()
}

fn cursor_mcp_server_type(server_type: ManagedAgentMcpServerType) -> &'static str {
    match server_type {
        ManagedAgentMcpServerType::Url => "http",
    }
}

fn prompt_from_events(events: &[UserEvent]) -> Result<Value, AgentSdkError> {
    let mut text = Vec::new();
    let mut images = Vec::new();
    for event in events {
        let UserEvent::Message { content } = event else {
            continue;
        };
        for block in content {
            match block {
                UserContentBlock::Text { text: value } => text.push(value.to_owned()),
                UserContentBlock::Image { url, source } => {
                    images.push(cursor_image_block(url.as_deref(), source.as_ref())?)
                }
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

fn cursor_image_block(
    url: Option<&str>,
    source: Option<&ImageSource>,
) -> Result<Value, AgentSdkError> {
    if let Some(url) = url {
        return Ok(json!({ "url": url }));
    }
    if let Some(source) = source {
        return Ok(json!({
            "data": source.data,
            "mimeType": source.mime_type,
        }));
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::sdk::agents::{
        types::{
            AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
            CreateSessionParams, EnvironmentConfig, EnvironmentNetworking, ManagedAgentMcpServer,
            ManagedAgentMcpServerType, ManagedAgentTool, SendEventsParams, UserContentBlock,
            UserEvent,
        },
        DEFAULT_CURSOR_BASE_URL,
    };

    fn provider() -> CursorProvider {
        CursorProvider::new(ProviderConfig::new(
            AgentRuntime::Cursor,
            reqwest::Client::new(),
            "cursor-test".to_owned(),
            DEFAULT_CURSOR_BASE_URL.to_owned(),
        ))
    }

    #[test]
    fn exposes_supported_cursor_params() {
        let provider = provider();

        assert_eq!(
            provider.supported_managed_agents_create_agent_params(),
            CREATE_AGENT_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_create_environment_params(),
            CREATE_ENVIRONMENT_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_create_session_params(),
            CREATE_SESSION_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_send_events_params(),
            SEND_EVENTS_PARAMS
        );
    }

    #[test]
    fn transforms_agent_params_to_cursor_shape_only_for_supported_fields() {
        let mut metadata = HashMap::new();
        metadata.insert("ignored".to_owned(), "true".to_owned());

        let body = provider()
            .transform_managed_agents_create_agent_params(CreateAgentParams {
                lap_agent_runtime: AgentRuntime::Cursor,
                name: "Coding Assistant".to_owned(),
                model: AgentModel::from("composer-2"),
                system: "You are a coding assistant.".to_owned(),
                description: Some("Ignored by Cursor".to_owned()),
                tools: vec![ManagedAgentTool::AgentToolset20260401],
                mcp_servers: vec![ManagedAgentMcpServer {
                    name: "linear".to_owned(),
                    server_type: ManagedAgentMcpServerType::Url,
                    url: "https://mcp.linear.app/sse".to_owned(),
                }],
                metadata: Some(metadata),
            })
            .unwrap();

        assert_eq!(
            body,
            json!({
                "prompt": { "text": "You are a coding assistant." },
                "name": "Coding Assistant",
                "model": { "id": "composer-2" },
                "mcpServers": [{
                    "name": "linear",
                    "type": "http",
                    "url": "https://mcp.linear.app/sse"
                }]
            })
        );
        assert!(body.get("description").is_none());
        assert!(body.get("tools").is_none());
        assert!(body.get("metadata").is_none());
        assert!(body.get("envVars").is_none());
        assert!(body.get("resources").is_none());
    }

    #[test]
    fn transforms_cursor_environment_and_session_params_without_provider_escape_hatches() {
        let provider = provider();

        let environment = provider
            .transform_managed_agents_create_environment_params(CreateEnvironmentParams {
                lap_agent_runtime: AgentRuntime::Cursor,
                name: "quickstart-env".to_owned(),
                config: EnvironmentConfig::Cloud {
                    networking: EnvironmentNetworking::Unrestricted,
                },
                description: Some("Ignored by Cursor".to_owned()),
                scope: Some("workspace".to_owned()),
                metadata: None,
            })
            .unwrap();
        let session = provider
            .transform_managed_agents_create_session_params(CreateSessionParams {
                agent: "bc-00000000-0000-0000-0000-000000000001".into(),
                environment_id: "env_ignored".to_owned(),
                title: "Ignored by Cursor".to_owned(),
                lap_agent_runtime: Some(AgentRuntime::Cursor),
                metadata: None,
            })
            .unwrap();

        assert_eq!(environment, json!({ "id": "quickstart-env" }));
        assert_eq!(
            session,
            json!({ "id": "bc-00000000-0000-0000-0000-000000000001" })
        );
        assert!(session.get("environment_id").is_none());
        assert!(session.get("title").is_none());
        assert!(session.get("resources").is_none());
    }

    #[test]
    fn cursor_session_requires_runtime_agent_id() {
        let err = provider()
            .transform_managed_agents_create_session_params(CreateSessionParams {
                agent: "lap-agent-definition".into(),
                environment_id: "env_123".to_owned(),
                title: "Quickstart session".to_owned(),
                lap_agent_runtime: Some(AgentRuntime::Cursor),
                metadata: None,
            })
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("requires a Cursor runtime agent id"));
    }

    #[test]
    fn transforms_user_message_events_to_cursor_prompt() {
        let body = provider()
            .transform_managed_agents_send_events_params(SendEventsParams {
                events: vec![UserEvent::Message {
                    content: vec![
                        UserContentBlock::text("First"),
                        UserContentBlock::text("Second"),
                        UserContentBlock::image_url("https://example.com/screenshot.png"),
                    ],
                }],
            })
            .unwrap();

        assert_eq!(
            body,
            json!({
                "prompt": {
                    "text": "First\n\nSecond",
                    "images": [{ "url": "https://example.com/screenshot.png" }]
                }
            })
        );
    }
}
