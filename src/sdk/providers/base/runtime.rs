//! Base contract for managed-agent runtime adapters.
//!
//! Provider runtimes such as Claude Managed Agents and Cursor implement this
//! trait so the SDK client can stay runtime-agnostic.

use std::{future::Future, pin::Pin, sync::Arc};

use serde_json::Value;

use crate::sdk::agents::{
    AgentEventStream, AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Environment, Lap, ManagedAgent, ManagedSessionRef, SendEventsParams,
    SendEventsResponse, Session, SessionContext,
};

pub(crate) type AdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentSdkError>> + Send + 'a>>;

pub(crate) struct RuntimeEntry {
    pub(crate) runtime: AgentRuntime,
    /// String ID stored in the database (e.g. "cursor", "claude_agents").
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) default_api_base: &'static str,
    pub(crate) adapter: Arc<dyn RuntimeAdapter>,
}

#[derive(Default)]
pub(crate) struct RuntimeAdapterRegistry {
    entries: Vec<RuntimeEntry>,
}

impl RuntimeAdapterRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(
        &mut self,
        runtime: AgentRuntime,
        id: &'static str,
        name: &'static str,
        default_api_base: &'static str,
        adapter: impl RuntimeAdapter,
    ) {
        self.entries.push(RuntimeEntry {
            runtime,
            id,
            name,
            default_api_base,
            adapter: Arc::new(adapter),
        });
    }

    pub(crate) fn get(&self, runtime: AgentRuntime) -> Option<Arc<dyn RuntimeAdapter>> {
        self.entries
            .iter()
            .find(|e| e.runtime == runtime)
            .map(|e| e.adapter.clone())
    }

    pub(crate) fn entry_for_id(&self, id: &str) -> Option<&RuntimeEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub(crate) fn validate_id(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    pub(crate) fn all_entries(&self) -> &[RuntimeEntry] {
        &self.entries
    }
}

pub(crate) trait RuntimeAdapter: Send + Sync + 'static {
    fn configure_request(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder;

    fn normalize_stream(&self, stream: AgentEventStream) -> AgentEventStream {
        stream
    }

    fn session_context(&self, session: ManagedSessionRef) -> SessionContext {
        SessionContext {
            runtime: session.lap_agent_runtime,
            provider_session_id: session.provider_session_id,
            agent_id: session.provider_agent_id,
            run_id: session.provider_run_id,
        }
    }

    /// Extract a provider-specific run ID from the raw `agents.create` response.
    /// Returns `None` by default; override for runtimes that return a run on creation.
    fn provider_run_id_from_agent_raw(&self, _raw: &Value) -> Option<String> {
        None
    }

    /// Extract a provider-specific URL from the raw `agents.create` response.
    /// Returns `None` by default.
    fn provider_url_from_agent_raw(&self, _raw: &Value) -> Option<String> {
        None
    }

    /// Return the agent ID to store when registering a session.
    /// For runtimes where the session ID doubles as the agent ID (e.g. Cursor),
    /// override to return `Some(provider_session_id)`.
    fn provider_agent_id_from_session_id(&self, _provider_session_id: &str) -> Option<String> {
        None
    }

    fn create_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateAgentParams,
    ) -> AdapterFuture<'a, ManagedAgent>;

    fn create_environment<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateEnvironmentParams,
    ) -> AdapterFuture<'a, Environment>;

    fn create_session<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateSessionParams,
    ) -> AdapterFuture<'a, Session>;

    fn send_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
        params: SendEventsParams,
    ) -> AdapterFuture<'a, SendEventsResponse>;

    fn stream_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
    ) -> AdapterFuture<'a, AgentEventStream>;
}
