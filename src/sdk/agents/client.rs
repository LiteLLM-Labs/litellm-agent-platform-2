use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use reqwest::{header, Method};
use serde::Serialize;
use serde_json::Value;

use super::{
    cursor_stream::normalize_cursor_stream,
    events::{stream_events, AgentEventStream},
    resources::Beta,
    responses::{ensure_success, response_json},
    types::{
        AgentRuntime, AgentSdkError, LapConfig, ManagedSessionRef, ANTHROPIC_VERSION,
        MANAGED_AGENTS_BETA,
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
pub(super) struct SessionContext {
    pub(super) runtime: AgentRuntime,
    pub(super) provider_session_id: Option<String>,
    pub(super) agent_id: Option<String>,
    pub(super) run_id: Option<String>,
}

impl Lap {
    pub fn new(config: LapConfig) -> Self {
        Self::with_http(configured_http_client(), runtime_configs(config))
    }

    pub(crate) fn with_http_client(config: LapConfig, http: reqwest::Client) -> Self {
        Self::with_http(http, runtime_configs(config))
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

    pub(super) async fn post<T: Serialize>(
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

    pub(super) async fn stream(
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

    pub(super) fn request(
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

    pub(super) fn default_runtime(&self) -> Result<AgentRuntime, AgentSdkError> {
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

    pub(super) fn runtime_for_session(
        &self,
        session_id: &str,
    ) -> Result<AgentRuntime, AgentSdkError> {
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

    pub(super) fn context_for_session(
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

    pub(super) fn remember_cursor_run(
        &self,
        agent_id: &str,
        run_id: &str,
    ) -> Result<(), AgentSdkError> {
        self.inner
            .cursor_run_ids
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(agent_id.to_owned(), run_id.to_owned());
        Ok(())
    }

    pub(super) fn cursor_run_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<String>, AgentSdkError> {
        Ok(self
            .inner
            .cursor_run_ids
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .get(agent_id)
            .cloned())
    }

    pub(super) fn remember_session_context(
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

    pub(super) fn remember_session(
        &self,
        session_id: &str,
        runtime: AgentRuntime,
    ) -> Result<(), AgentSdkError> {
        self.remember_session_context(
            session_id,
            SessionContext {
                runtime,
                provider_session_id: Some(session_id.to_owned()),
                agent_id: None,
                run_id: None,
            },
        )
    }
}

impl SessionContext {
    pub(super) fn cursor(agent_id: String, run_id: Option<String>) -> Self {
        Self {
            runtime: AgentRuntime::Cursor,
            provider_session_id: Some(agent_id.clone()),
            agent_id: Some(agent_id),
            run_id,
        }
    }
}

fn runtime_configs(config: LapConfig) -> HashMap<AgentRuntime, RuntimeConfig> {
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
    runtimes
}

fn configured_http_client() -> reqwest::Client {
    reqwest::Client::new()
}
