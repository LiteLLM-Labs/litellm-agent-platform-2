use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use reqwest::{header, Method};
use serde::Serialize;
use serde_json::Value;

use super::{
    events::{stream_events, AgentEventStream},
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, LapConfig, ManagedAgent, SendEventsParams,
        SendEventsResponse, Session, ANTHROPIC_VERSION, MANAGED_AGENTS_BETA,
    },
};

#[derive(Clone)]
pub struct Lap {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    runtimes: HashMap<AgentRuntime, RuntimeConfig>,
    session_runtimes: Mutex<HashMap<String, AgentRuntime>>,
}

#[derive(Debug, Clone)]
struct RuntimeConfig {
    api_key: String,
    base_url: String,
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
        Self::with_http(configured_http_client(), runtimes)
    }

    fn with_http(http: reqwest::Client, runtimes: HashMap<AgentRuntime, RuntimeConfig>) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                runtimes,
                session_runtimes: Mutex::new(HashMap::new()),
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
        ensure_success(response).await.map(stream_events)
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
        Ok(self
            .inner
            .http
            .request(method, format!("{}{}", config.base_url, path))
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-beta", MANAGED_AGENTS_BETA))
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
        let sessions = self
            .inner
            .session_runtimes
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?;
        sessions
            .get(session_id)
            .copied()
            .map(Ok)
            .unwrap_or_else(|| self.default_runtime())
    }

    fn remember_session(
        &self,
        session_id: &str,
        runtime: AgentRuntime,
    ) -> Result<(), AgentSdkError> {
        self.inner
            .session_runtimes
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(session_id.to_owned(), runtime);
        Ok(())
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
        let raw = self.client.post(runtime, "/v1/agents", &params).await?;
        Ok(ManagedAgent {
            id: id(&raw)?,
            version: raw.get("version").and_then(Value::as_u64),
            raw,
        })
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
        let raw = self
            .client
            .post(runtime, "/v1/environments", &params)
            .await?;
        Ok(Environment { id: id(&raw)?, raw })
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
        let raw = self.client.post(runtime, "/v1/sessions", &params).await?;
        let session = Session { id: id(&raw)?, raw };
        self.client.remember_session(&session.id, runtime)?;
        Ok(session)
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
        let raw = self
            .client
            .post(
                runtime,
                &format!("/v1/sessions/{session_id}/events"),
                &params,
            )
            .await?;
        Ok(SendEventsResponse { raw })
    }

    pub async fn stream(&self, session_id: &str) -> Result<AgentEventStream, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        self.client
            .stream(runtime, &format!("/v1/sessions/{session_id}/events/stream"))
            .await
    }
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
