use futures_util::future::BoxFuture;
use reqwest::{header, Method};
use serde::Serialize;

use super::super::transform::{
    ensure_success, response_json, string_field, AgentRuntimeProvider, ProviderConfig,
    ProviderSendEventsResponse, ProviderSession, ProviderSessionContext,
};
use crate::sdk::agents::{
    events::{stream_events, AgentEventStream},
    types::{
        AgentSdkError, CreateAgentParams, CreateEnvironmentParams, CreateSessionParams,
        Environment, ManagedAgent, SendEventsParams, SendEventsResponse, Session,
        ANTHROPIC_VERSION, MANAGED_AGENTS_BETA,
    },
};

#[derive(Debug, Clone)]
pub struct ClaudeManagedAgentsProvider {
    config: ProviderConfig,
}

impl ClaudeManagedAgentsProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    async fn post<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<serde_json::Value, AgentSdkError> {
        let response = self.request(Method::POST, path).json(body).send().await?;
        response_json(response).await
    }

    async fn stream(&self, path: &str) -> Result<AgentEventStream, AgentSdkError> {
        let response = self
            .request(Method::GET, path)
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;
        ensure_success(response).await.map(stream_events)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.config
            .http
            .request(method, self.config.url(path))
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-beta", MANAGED_AGENTS_BETA)
    }
}

impl AgentRuntimeProvider for ClaudeManagedAgentsProvider {
    fn create_agent<'a>(
        &'a self,
        params: CreateAgentParams,
    ) -> BoxFuture<'a, Result<ManagedAgent, AgentSdkError>> {
        Box::pin(async move {
            let raw = self.post("/v1/agents", &params).await?;
            Ok(ManagedAgent {
                id: string_field(&raw, "id").map_err(|_| AgentSdkError::MissingId)?,
                version: raw.get("version").and_then(serde_json::Value::as_u64),
                raw,
            })
        })
    }

    fn create_environment<'a>(
        &'a self,
        params: CreateEnvironmentParams,
    ) -> BoxFuture<'a, Result<Environment, AgentSdkError>> {
        Box::pin(async move {
            let raw = self.post("/v1/environments", &params).await?;
            Ok(Environment {
                id: string_field(&raw, "id").map_err(|_| AgentSdkError::MissingId)?,
                raw,
            })
        })
    }

    fn create_session<'a>(
        &'a self,
        params: CreateSessionParams,
    ) -> BoxFuture<'a, Result<ProviderSession, AgentSdkError>> {
        Box::pin(async move {
            let raw = self.post("/v1/sessions", &params).await?;
            let id = string_field(&raw, "id").map_err(|_| AgentSdkError::MissingId)?;
            Ok(ProviderSession {
                session: Session {
                    id: id.clone(),
                    raw,
                },
                context: ProviderSessionContext {
                    runtime: self.config.runtime,
                    agent_id: None,
                    run_id: Some(id),
                },
            })
        })
    }

    fn send_events<'a>(
        &'a self,
        session_id: &'a str,
        params: SendEventsParams,
        _context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<ProviderSendEventsResponse, AgentSdkError>> {
        Box::pin(async move {
            let raw = self
                .post(&format!("/v1/sessions/{session_id}/events"), &params)
                .await?;
            Ok(ProviderSendEventsResponse {
                response: SendEventsResponse { raw },
                context: None,
            })
        })
    }

    fn stream_events<'a>(
        &'a self,
        session_id: &'a str,
        _context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<AgentEventStream, AgentSdkError>> {
        Box::pin(async move {
            self.stream(&format!("/v1/sessions/{session_id}/events/stream"))
                .await
        })
    }
}
