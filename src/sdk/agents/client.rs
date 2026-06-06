use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use async_stream::try_stream;
use futures_util::StreamExt;
use reqwest::{header, Method};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{
    events::{stream_events, AgentEvent, AgentEventStream},
    types::{
        AgentModel, AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, LapConfig, ManagedAgent, ManagedSessionRef,
        SendEventsParams, SendEventsResponse, Session, ANTHROPIC_VERSION, MANAGED_AGENTS_BETA,
    },
};

#[derive(Clone)]
pub struct Lap {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    runtimes: HashMap<AgentRuntime, RuntimeConfig>,
    session_contexts: Mutex<HashMap<String, SessionContext>>,
    cursor_run_ids: Mutex<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct RuntimeConfig {
    api_key: String,
    base_url: String,
}

#[derive(Debug, Clone)]
struct SessionContext {
    runtime: AgentRuntime,
    provider_session_id: Option<String>,
    agent_id: Option<String>,
    run_id: Option<String>,
}

impl Lap {
    pub fn new(config: LapConfig) -> Self {
        let mut runtimes = HashMap::new();
        if let Some(api_key) = config.anthropic_api_key {
            runtimes.insert(
                AgentRuntime::ClaudeManagedAgents,
                RuntimeConfig {
                    api_key,
                    base_url: config.anthropic_base_url.trim_end_matches('/').to_owned(),
                },
            );
        }
        if let Some(api_key) = config.cursor_api_key {
            runtimes.insert(
                AgentRuntime::Cursor,
                RuntimeConfig {
                    api_key,
                    base_url: config.cursor_base_url.trim_end_matches('/').to_owned(),
                },
            );
        }
        Self::with_http(configured_http_client(), runtimes)
    }

    pub(crate) fn with_http_client(config: LapConfig, http: reqwest::Client) -> Self {
        let mut runtimes = HashMap::new();
        if let Some(api_key) = config.anthropic_api_key {
            runtimes.insert(
                AgentRuntime::ClaudeManagedAgents,
                RuntimeConfig {
                    api_key,
                    base_url: config.anthropic_base_url.trim_end_matches('/').to_owned(),
                },
            );
        }
        if let Some(api_key) = config.cursor_api_key {
            runtimes.insert(
                AgentRuntime::Cursor,
                RuntimeConfig {
                    api_key,
                    base_url: config.cursor_base_url.trim_end_matches('/').to_owned(),
                },
            );
        }
        Self::with_http(http, runtimes)
    }

    pub fn register_session(&self, session: ManagedSessionRef) -> Result<(), AgentSdkError> {
        let ManagedSessionRef {
            session_id,
            lap_agent_runtime,
            provider_session_id,
            provider_agent_id,
            provider_run_id,
        } = session;
        let agent_id = match lap_agent_runtime {
            AgentRuntime::Cursor => provider_agent_id.or_else(|| provider_session_id.clone()),
            AgentRuntime::ClaudeManagedAgents => provider_agent_id,
        };
        self.remember_session_context(
            &session_id,
            SessionContext {
                runtime: lap_agent_runtime,
                provider_session_id,
                agent_id,
                run_id: provider_run_id,
            },
        )
    }

    fn with_http(http: reqwest::Client, runtimes: HashMap<AgentRuntime, RuntimeConfig>) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                runtimes,
                session_contexts: Mutex::new(HashMap::new()),
                cursor_run_ids: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn beta(&self) -> Beta<'_> {
        Beta { client: self }
    }

    async fn post<T: Serialize>(
        &self,
        runtime: AgentRuntime,
        path: &str,
        body: &T,
    ) -> Result<Value, AgentSdkError> {
        let response = self
            .request(runtime, Method::POST, path)?
            .json(body)
            .send()
            .await?;
        response_json(response).await
    }

    async fn stream(
        &self,
        runtime: AgentRuntime,
        path: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let response = self
            .request(runtime, Method::GET, path)?
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        let stream = stream_events(ensure_success(response).await?);
        match runtime {
            AgentRuntime::ClaudeManagedAgents => Ok(stream),
            AgentRuntime::Cursor => Ok(normalize_cursor_stream(stream)),
        }
    }

    fn request(
        &self,
        runtime: AgentRuntime,
        method: Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, AgentSdkError> {
        let config = self
            .inner
            .runtimes
            .get(&runtime)
            .ok_or(AgentSdkError::RuntimeNotConfigured(runtime))?;
        let request = self
            .inner
            .http
            .request(method, format!("{}{}", config.base_url, path))
            .header(header::CONTENT_TYPE, "application/json");
        Ok(match runtime {
            AgentRuntime::ClaudeManagedAgents => request
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("anthropic-beta", MANAGED_AGENTS_BETA),
            AgentRuntime::Cursor => request.bearer_auth(&config.api_key),
        })
    }

    fn default_runtime(&self) -> Result<AgentRuntime, AgentSdkError> {
        if self.inner.runtimes.len() == 1 {
            self.inner
                .runtimes
                .keys()
                .copied()
                .next()
                .ok_or(AgentSdkError::NoRuntimesConfigured)
        } else if self.inner.runtimes.is_empty() {
            Err(AgentSdkError::NoRuntimesConfigured)
        } else {
            Err(AgentSdkError::RuntimeRequired)
        }
    }

    fn runtime_for_session(&self, session_id: &str) -> Result<AgentRuntime, AgentSdkError> {
        let contexts = self
            .inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?;
        contexts
            .get(session_id)
            .map(|context| context.runtime)
            .map(Ok)
            .unwrap_or_else(|| self.default_runtime())
    }

    fn context_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionContext>, AgentSdkError> {
        let contexts = self
            .inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?;
        Ok(contexts.get(session_id).cloned())
    }

    fn remember_cursor_run(&self, agent_id: &str, run_id: &str) -> Result<(), AgentSdkError> {
        self.inner
            .cursor_run_ids
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(agent_id.to_owned(), run_id.to_owned());
        Ok(())
    }

    fn cursor_run_for_agent(&self, agent_id: &str) -> Result<Option<String>, AgentSdkError> {
        Ok(self
            .inner
            .cursor_run_ids
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .get(agent_id)
            .cloned())
    }

    fn remember_session_context(
        &self,
        session_id: &str,
        context: SessionContext,
    ) -> Result<(), AgentSdkError> {
        self.inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(session_id.to_owned(), context);
        Ok(())
    }

    fn remember_session(
        &self,
        session_id: &str,
        runtime: AgentRuntime,
    ) -> Result<(), AgentSdkError> {
        self.inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(
                session_id.to_owned(),
                SessionContext {
                    runtime,
                    provider_session_id: Some(session_id.to_owned()),
                    agent_id: None,
                    run_id: None,
                },
            );
        Ok(())
    }
}

impl SessionContext {
    fn cursor(agent_id: String, run_id: Option<String>) -> Self {
        Self {
            runtime: AgentRuntime::Cursor,
            provider_session_id: Some(agent_id.clone()),
            agent_id: Some(agent_id),
            run_id,
        }
    }
}

pub struct Beta<'a> {
    client: &'a Lap,
}

impl<'a> Beta<'a> {
    pub fn agents(&self) -> Agents<'a> {
        Agents {
            client: self.client,
        }
    }

    pub fn environments(&self) -> Environments<'a> {
        Environments {
            client: self.client,
        }
    }

    pub fn sessions(&self) -> Sessions<'a> {
        Sessions {
            client: self.client,
        }
    }
}

pub struct Agents<'a> {
    client: &'a Lap,
}

impl Agents<'_> {
    pub async fn create(&self, params: CreateAgentParams) -> Result<ManagedAgent, AgentSdkError> {
        let runtime = params.lap_agent_runtime;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let raw = self.client.post(runtime, "/v1/agents", &params).await?;
                Ok(ManagedAgent {
                    id: id(&raw)?,
                    version: raw.get("version").and_then(Value::as_u64),
                    raw,
                })
            }
            AgentRuntime::Cursor => {
                let raw = self
                    .client
                    .post(runtime, "/v1/agents", &cursor_create_agent_body(params))
                    .await?;
                let agent_id = nested_id(&raw, "agent")?;
                if let Some(run_id) = cursor_run_id(&raw) {
                    self.client.remember_cursor_run(&agent_id, &run_id)?;
                }
                Ok(ManagedAgent {
                    id: agent_id,
                    version: None,
                    raw,
                })
            }
        }
    }
}

pub struct Environments<'a> {
    client: &'a Lap,
}

impl Environments<'_> {
    pub async fn create(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<Environment, AgentSdkError> {
        let runtime = params.lap_agent_runtime;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let raw = self
                    .client
                    .post(runtime, "/v1/environments", &params)
                    .await?;
                Ok(Environment { id: id(&raw)?, raw })
            }
            AgentRuntime::Cursor => {
                let raw = json!({ "id": params.name });
                Ok(Environment { id: id(&raw)?, raw })
            }
        }
    }
}

pub struct Sessions<'a> {
    client: &'a Lap,
}

impl<'a> Sessions<'a> {
    pub async fn create(&self, params: CreateSessionParams) -> Result<Session, AgentSdkError> {
        let runtime = params
            .lap_agent_runtime
            .map(Ok)
            .unwrap_or_else(|| self.client.default_runtime())?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let raw = self.client.post(runtime, "/v1/sessions", &params).await?;
                let session = Session { id: id(&raw)?, raw };
                self.client.remember_session(&session.id, runtime)?;
                Ok(session)
            }
            AgentRuntime::Cursor => {
                if params.agent.trim().is_empty() {
                    return Err(AgentSdkError::InvalidRequest(
                        "cursor sessions.create requires a non-empty Cursor agent id".to_owned(),
                    ));
                }
                let raw = json!({ "id": params.agent });
                let session = Session { id: id(&raw)?, raw };
                let run_id = self.client.cursor_run_for_agent(&session.id)?;
                self.client.remember_session_context(
                    &session.id,
                    SessionContext::cursor(session.id.clone(), run_id),
                )?;
                Ok(session)
            }
        }
    }

    pub fn events(&self) -> SessionEvents<'a> {
        SessionEvents {
            client: self.client,
        }
    }
}

pub struct SessionEvents<'a> {
    client: &'a Lap,
}

impl SessionEvents<'_> {
    pub async fn send(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let provider_session_id = self.provider_session_id(session_id)?;
                let raw = self
                    .client
                    .post(
                        runtime,
                        &format!("/v1/sessions/{provider_session_id}/events"),
                        &params,
                    )
                    .await?;
                Ok(SendEventsResponse { raw })
            }
            AgentRuntime::Cursor => {
                let agent_id = self.cursor_agent_id(session_id)?;
                let body = json!({ "prompt": cursor_prompt_from_events(&params.events)? });
                let raw = self
                    .client
                    .post(runtime, &format!("/v1/agents/{agent_id}/runs"), &body)
                    .await?;
                let run_id = nested_string_field(&raw, "run", "id")?;
                self.client.remember_session_context(
                    session_id,
                    SessionContext::cursor(agent_id, Some(run_id)),
                )?;
                Ok(SendEventsResponse { raw })
            }
        }
    }

    pub async fn stream(&self, session_id: &str) -> Result<AgentEventStream, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let provider_session_id = self.provider_session_id(session_id)?;
                self.client
                    .stream(
                        runtime,
                        &format!("/v1/sessions/{provider_session_id}/events/stream"),
                    )
                    .await
            }
            AgentRuntime::Cursor => {
                let context = self.client.context_for_session(session_id)?;
                let agent_id = cursor_agent_id_from_context(session_id, context.as_ref());
                let run_id = match context.and_then(|context| context.run_id) {
                    Some(run_id) => run_id,
                    None => self.latest_cursor_run_id(&agent_id).await?,
                };
                self.client
                    .stream(
                        runtime,
                        &format!("/v1/agents/{agent_id}/runs/{run_id}/stream"),
                    )
                    .await
            }
        }
    }

    fn provider_session_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(self
            .client
            .context_for_session(session_id)?
            .and_then(|context| context.provider_session_id)
            .unwrap_or_else(|| session_id.to_owned()))
    }

    fn cursor_agent_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(cursor_agent_id_from_context(
            session_id,
            self.client.context_for_session(session_id)?.as_ref(),
        ))
    }

    async fn latest_cursor_run_id(&self, agent_id: &str) -> Result<String, AgentSdkError> {
        let response = self
            .client
            .request(
                AgentRuntime::Cursor,
                Method::GET,
                &format!("/v1/agents/{agent_id}"),
            )?
            .send()
            .await?;
        let raw = response_json(response).await?;
        cursor_run_id(&raw).ok_or(AgentSdkError::MissingField("latestRunId"))
    }
}

fn cursor_agent_id_from_context(session_id: &str, context: Option<&SessionContext>) -> String {
    context
        .and_then(|context| context.agent_id.clone())
        .or_else(|| context.and_then(|context| context.provider_session_id.clone()))
        .unwrap_or_else(|| session_id.to_owned())
}

fn configured_http_client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn response_json(response: reqwest::Response) -> Result<Value, AgentSdkError> {
    let response = ensure_success(response).await?;
    let text = response.text().await?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(&text).map_err(AgentSdkError::Json)
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, AgentSdkError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AgentSdkError::Provider { status, body })
}

fn id(raw: &Value) -> Result<String, AgentSdkError> {
    raw.get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(AgentSdkError::MissingId)
}

fn nested_id(raw: &Value, parent: &'static str) -> Result<String, AgentSdkError> {
    raw.get(parent)
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(AgentSdkError::MissingId)
}

fn nested_string_field(
    raw: &Value,
    parent: &'static str,
    field: &'static str,
) -> Result<String, AgentSdkError> {
    raw.get(parent)
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(AgentSdkError::MissingField(field))
}

fn cursor_run_id(raw: &Value) -> Option<String> {
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

fn cursor_create_agent_body(params: CreateAgentParams) -> Value {
    let mut body = Map::new();
    body.insert("prompt".to_owned(), json!({ "text": params.system }));
    body.insert("name".to_owned(), Value::String(params.name));
    body.insert("model".to_owned(), cursor_model(params.model));
    if let Some(Value::Object(options)) = params.lap_provider_options {
        for (key, value) in options {
            body.insert(key, value);
        }
    }
    if !params.mcp_servers.is_empty() {
        body.insert(
            "mcpServers".to_owned(),
            Value::Array(
                params
                    .mcp_servers
                    .into_iter()
                    .map(cursor_mcp_server)
                    .collect(),
            ),
        );
    }
    Value::Object(body)
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

fn cursor_mcp_server(server: Value) -> Value {
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

fn cursor_prompt_from_events(events: &[Value]) -> Result<Value, AgentSdkError> {
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

fn normalize_cursor_stream(mut stream: AgentEventStream) -> AgentEventStream {
    let stream = try_stream! {
        let mut state = CursorStreamState::default();
        while let Some(event) = stream.next().await {
            for event in state.normalize(event?) {
                yield event;
            }
        }
        if let Some(event) = state.flush_agent_message() {
            yield event;
        }
    };
    Box::pin(stream)
}

#[derive(Default)]
struct CursorStreamState {
    assistant_text: String,
    emitted_running: bool,
    emitted_thinking: bool,
    emitted_tool_uses: HashSet<String>,
}

impl CursorStreamState {
    fn normalize(&mut self, event: AgentEvent) -> Vec<AgentEvent> {
        match event.event_type.as_str() {
            "assistant" => {
                if let Some(text) = event.data.get("text").and_then(Value::as_str) {
                    self.assistant_text.push_str(text);
                }
                Vec::new()
            }
            "thinking" => self.thinking_event().into_iter().collect(),
            "tool_call" => self.tool_events(event.data),
            "status" | "result" => self.status_events(event.data),
            "error" => vec![simple_event("session.error", event.data)],
            "done" | "heartbeat" => Vec::new(),
            _ => Vec::new(),
        }
    }

    fn status_events(&mut self, data: Map<String, Value>) -> Vec<AgentEvent> {
        match data.get("status").and_then(Value::as_str) {
            Some("RUNNING") => self.running_event().into_iter().collect(),
            Some("FINISHED") => self.flush_then(simple_event("session.status_idle", idle_data())),
            Some("ERROR") | Some("CANCELLED") | Some("EXPIRED") => {
                self.flush_then(simple_event("session.error", data))
            }
            _ => Vec::new(),
        }
    }

    fn thinking_event(&mut self) -> Option<AgentEvent> {
        if self.emitted_thinking {
            return None;
        }
        self.emitted_thinking = true;
        Some(simple_event("agent.thinking", Map::new()))
    }

    fn tool_events(&mut self, data: Map<String, Value>) -> Vec<AgentEvent> {
        let status = data.get("status").and_then(Value::as_str);
        if status == Some("completed") {
            return self.completed_tool_events(data);
        }
        if status == Some("running") && data.get("args").is_some() {
            return self
                .emit_tool_use(data)
                .map(|event| self.flush_then(event))
                .unwrap_or_default();
        }
        Vec::new()
    }

    fn completed_tool_events(&mut self, data: Map<String, Value>) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if let Some(event) = self.emit_tool_use(data.clone()) {
            events.extend(self.flush_then(event));
        }
        if let Some(event) = tool_result_event(data) {
            events.push(event);
        }
        events
    }

    fn emit_tool_use(&mut self, data: Map<String, Value>) -> Option<AgentEvent> {
        let call_id = data.get("callId").and_then(Value::as_str)?;
        if !self.emitted_tool_uses.insert(call_id.to_owned()) {
            return None;
        }
        Some(tool_event(data))
    }

    fn running_event(&mut self) -> Option<AgentEvent> {
        if self.emitted_running {
            return None;
        }
        self.emitted_running = true;
        Some(simple_event("session.status_running", Map::new()))
    }

    fn flush_then(&mut self, event: AgentEvent) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if let Some(message) = self.flush_agent_message() {
            events.push(message);
        }
        events.push(event);
        events
    }

    fn flush_agent_message(&mut self) -> Option<AgentEvent> {
        if self.assistant_text.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.assistant_text);
        Some(agent_message_event(text))
    }
}

fn agent_message_event(text: String) -> AgentEvent {
    let mut data = Map::new();
    data.insert(
        "content".to_owned(),
        json!([{ "type": "text", "text": text }]),
    );
    simple_event("agent.message", data)
}

fn tool_event(mut data: Map<String, Value>) -> AgentEvent {
    if let Some(call_id) = data.remove("callId") {
        data.insert("id".to_owned(), call_id);
    }
    if let Some(args) = data.remove("args") {
        data.insert("input".to_owned(), args);
    }
    data.remove("status");
    data.remove("result");
    data.entry("input".to_owned()).or_insert_with(|| json!({}));
    simple_event("agent.tool_use", data)
}

fn tool_result_event(mut data: Map<String, Value>) -> Option<AgentEvent> {
    let call_id = data.remove("callId")?;
    let result = data.remove("result");
    data.clear();
    data.insert("tool_use_id".to_owned(), call_id);
    if let Some(result) = result {
        data.insert("content".to_owned(), json!([text_block(result)]));
    }
    Some(simple_event("agent.tool_result", data))
}

fn text_block(value: Value) -> Value {
    match value {
        Value::String(text) => json!({ "type": "text", "text": text }),
        value => json!({ "type": "text", "text": value.to_string() }),
    }
}

fn idle_data() -> Map<String, Value> {
    let mut data = Map::new();
    data.insert("stop_reason".to_owned(), json!({ "type": "end_turn" }));
    data
}

fn simple_event(event_type: &str, data: Map<String, Value>) -> AgentEvent {
    AgentEvent {
        event_type: event_type.to_owned(),
        data,
    }
}
