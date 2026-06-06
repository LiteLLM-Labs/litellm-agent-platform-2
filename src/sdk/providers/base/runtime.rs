//! Base contract for managed-agent runtime adapters.
//!
//! Provider runtimes such as Claude Managed Agents and Cursor implement this
//! trait so the SDK client can stay runtime-agnostic.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use crate::sdk::agents::{
    AgentEventStream, AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Environment, Lap, ManagedAgent, ManagedSessionRef, SendEventsParams,
    SendEventsResponse, Session, SessionContext,
};

pub(crate) type AdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentSdkError>> + Send + 'a>>;

#[derive(Default)]
pub(crate) struct RuntimeAdapterRegistry {
    adapters: HashMap<AgentRuntime, Arc<dyn RuntimeAdapter>>,
}

impl RuntimeAdapterRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(&mut self, runtime: AgentRuntime, adapter: impl RuntimeAdapter) {
        self.adapters.insert(runtime, Arc::new(adapter));
    }

    pub(crate) fn get(&self, runtime: AgentRuntime) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters.get(&runtime).cloned()
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
