use std::{collections::HashMap, sync::Arc};

use futures_util::future::BoxFuture;
use serde_json::Value;

use super::super::{
    events::AgentEventStream,
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, SendEventsParams, SendEventsResponse,
        Session,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct ProviderConfig {
    pub runtime: AgentRuntime,
    pub http: reqwest::Client,
    pub api_key: String,
    pub base_url: String,
}

impl ProviderConfig {
    pub fn new(
        runtime: AgentRuntime,
        http: reqwest::Client,
        api_key: String,
        base_url: String,
    ) -> Self {
        Self {
            runtime,
            http,
            api_key,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderSessionContext {
    pub runtime: AgentRuntime,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderSession {
    pub session: Session,
    pub context: ProviderSessionContext,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderSendEventsResponse {
    pub response: SendEventsResponse,
    pub context: Option<ProviderSessionContext>,
}

pub(crate) trait AgentRuntimeProvider: Send + Sync + 'static {
    fn supported_managed_agents_create_agent_params(&self) -> &'static [&'static str];

    fn transform_managed_agents_create_agent_params(
        &self,
        params: CreateAgentParams,
    ) -> Result<Value, AgentSdkError>;

    fn create_agent<'a>(
        &'a self,
        params: CreateAgentParams,
    ) -> BoxFuture<'a, Result<ManagedAgent, AgentSdkError>>;

    fn supported_managed_agents_create_environment_params(&self) -> &'static [&'static str];

    fn transform_managed_agents_create_environment_params(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<Value, AgentSdkError>;

    fn create_environment<'a>(
        &'a self,
        params: CreateEnvironmentParams,
    ) -> BoxFuture<'a, Result<Environment, AgentSdkError>>;

    fn supported_managed_agents_create_session_params(&self) -> &'static [&'static str];

    fn transform_managed_agents_create_session_params(
        &self,
        params: CreateSessionParams,
    ) -> Result<Value, AgentSdkError>;

    fn create_session<'a>(
        &'a self,
        params: CreateSessionParams,
    ) -> BoxFuture<'a, Result<ProviderSession, AgentSdkError>>;

    fn supported_managed_agents_send_events_params(&self) -> &'static [&'static str];

    fn transform_managed_agents_send_events_params(
        &self,
        params: SendEventsParams,
    ) -> Result<Value, AgentSdkError>;

    fn send_events<'a>(
        &'a self,
        session_id: &'a str,
        params: SendEventsParams,
        context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<ProviderSendEventsResponse, AgentSdkError>>;

    fn stream_events<'a>(
        &'a self,
        session_id: &'a str,
        context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<AgentEventStream, AgentSdkError>>;
}

#[derive(Clone)]
pub(crate) struct AgentProvider {
    pub handler: Arc<dyn AgentRuntimeProvider>,
}

#[derive(Default, Clone)]
pub(crate) struct AgentProviderRegistry {
    providers: HashMap<AgentRuntime, AgentProvider>,
}

impl AgentProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, runtime: AgentRuntime, handler: impl AgentRuntimeProvider) {
        self.providers.insert(
            runtime,
            AgentProvider {
                handler: Arc::new(handler),
            },
        );
    }

    pub fn get(&self, runtime: AgentRuntime) -> Option<AgentProvider> {
        self.providers.get(&runtime).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn only_runtime(&self) -> Option<AgentRuntime> {
        if self.providers.len() == 1 {
            self.providers.keys().copied().next()
        } else {
            None
        }
    }
}

impl std::fmt::Debug for AgentProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProviderRegistry")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .finish()
    }
}

pub(crate) async fn response_json(response: reqwest::Response) -> Result<Value, AgentSdkError> {
    let response = ensure_success(response).await?;
    let text = response.text().await?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(&text).map_err(AgentSdkError::Json)
}

pub(crate) async fn ensure_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, AgentSdkError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AgentSdkError::Provider { status, body })
}

pub(crate) fn string_field(raw: &Value, field: &'static str) -> Result<String, AgentSdkError> {
    raw.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(AgentSdkError::MissingField(field))
}

pub(crate) fn nested_string_field(
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

pub(crate) fn default_if_empty(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_owned()
    } else {
        value.to_owned()
    }
}
