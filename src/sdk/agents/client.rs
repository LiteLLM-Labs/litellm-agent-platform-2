use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use reqwest::{header, Method, StatusCode};
use serde::Serialize;
use serde_json::Value;

use super::{
    events::{stream_events, AgentEventStream},
    resources::Beta,
    responses::{ensure_success, response_json},
    runtime_config::{configured_http_client, runtime_configs, RuntimeConfig},
    types::{AgentRuntime, AgentSdkError, LapConfig, ManagedSessionRef},
};
use crate::sdk::{providers, providers::base::runtime::RuntimeAdapter};

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
pub(crate) struct SessionContext {
    pub(crate) runtime: AgentRuntime,
    pub(crate) provider_session_id: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) run_id: Option<String>,
}

impl Lap {
    pub fn new(config: LapConfig) -> Self {
        Self::with_http(configured_http_client(), runtime_configs(config))
    }

    pub(crate) fn with_http_client(config: LapConfig, http: reqwest::Client) -> Self {
        Self::with_http(http, runtime_configs(config))
    }

    pub fn register_session(&self, session: ManagedSessionRef) -> Result<(), AgentSdkError> {
        let session_id = session.session_id.clone();
        let context = self
            .adapter(session.lap_agent_runtime)?
            .session_context(session);
        self.remember_session_context(&session_id, context)
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

    pub(crate) async fn post<T: Serialize>(
        &self,
        runtime: AgentRuntime,
        path: &str,
        body: &T,
    ) -> Result<Value, AgentSdkError> {
        let mut response = self
            .request(runtime, Method::POST, path)?
            .json(body)
            .send()
            .await?;
        if runtime == AgentRuntime::OpenCode && response.status() == StatusCode::UNAUTHORIZED {
            if let Some(request) = self.opencode_bearer_request(Method::POST, path)? {
                response = request.json(body).send().await?;
            }
        }
        response_json(response).await
    }

    pub(super) async fn get(
        &self,
        runtime: AgentRuntime,
        path: &str,
    ) -> Result<Value, AgentSdkError> {
        let mut response = self.request(runtime, Method::GET, path)?.send().await?;
        if runtime == AgentRuntime::OpenCode && response.status() == StatusCode::UNAUTHORIZED {
            if let Some(request) = self.opencode_bearer_request(Method::GET, path)? {
                response = request.send().await?;
            }
        }
        response_json(response).await
    }

    pub(crate) async fn stream(
        &self,
        runtime: AgentRuntime,
        path: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let mut response = self
            .request(runtime, Method::GET, path)?
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        if runtime == AgentRuntime::OpenCode && response.status() == StatusCode::UNAUTHORIZED {
            if let Some(request) = self.opencode_bearer_request(Method::GET, path)? {
                response = request
                    .header(header::ACCEPT, "text/event-stream")
                    .send()
                    .await?;
            }
        }
        let stream = stream_events(ensure_success(response).await?);
        // OpenCode stream normalization is handled in session_events.rs; skip adapter for it.
        if runtime == AgentRuntime::OpenCode {
            return Ok(stream);
        }
        Ok(self.adapter(runtime)?.normalize_stream(stream))
    }

    pub(crate) fn request(
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
        Ok(config.authorize(request))
    }

    fn opencode_bearer_request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<Option<reqwest::RequestBuilder>, AgentSdkError> {
        let config = self
            .inner
            .runtimes
            .get(&AgentRuntime::OpenCode)
            .ok_or(AgentSdkError::RuntimeNotConfigured(AgentRuntime::OpenCode))?;
        let request = self
            .inner
            .http
            .request(method, format!("{}{}", config.base_url, path))
            .header(header::CONTENT_TYPE, "application/json");
        Ok(config.authorize_opencode_bearer(request))
    }

    pub(super) fn adapter(
        &self,
        runtime: AgentRuntime,
    ) -> Result<Arc<dyn RuntimeAdapter>, AgentSdkError> {
        providers::adapter(runtime).ok_or(AgentSdkError::RuntimeNotConfigured(runtime))
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

    pub(crate) fn context_for_session(
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

    pub(crate) fn remember_cursor_run(
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

    pub(crate) fn cursor_run_for_agent(
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

    pub(crate) fn remember_session_context(
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

    pub(crate) fn remember_session(
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
    pub(crate) fn cursor(agent_id: String, run_id: Option<String>) -> Self {
        Self {
            runtime: AgentRuntime::Cursor,
            provider_session_id: Some(agent_id.clone()),
            agent_id: Some(agent_id),
            run_id,
        }
    }
}
