mod claude;
mod cursor;
mod cursor_stream;

use std::{future::Future, pin::Pin, sync::Arc};

use crate::sdk::agents::{
    client::{Lap, SessionContext},
    events::AgentEventStream,
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, ManagedSessionRef, SendEventsParams,
        SendEventsResponse, Session,
    },
};

pub(super) type AdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentSdkError>> + Send + 'a>>;

pub(super) trait RuntimeAdapter: Send + Sync {
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

pub(super) fn adapter(runtime: AgentRuntime) -> Arc<dyn RuntimeAdapter> {
    match runtime {
        AgentRuntime::ClaudeManagedAgents => Arc::new(claude::ClaudeManagedAgentsRuntime),
        AgentRuntime::Cursor => Arc::new(cursor::CursorRuntime),
    }
}
