//! Provider-owned SDK integrations.
//!
//! Each provider folder owns the target endpoints and runtimes it supports.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use axum::http::HeaderMap;
use serde_json::Value;

use crate::{
    errors::GatewayError,
    sdk::{
        agents::{
            AgentEventStream, AgentRuntime, AgentSdkError, CreateAgentParams,
            CreateEnvironmentParams, CreateSessionParams, Environment, Lap, ManagedAgent,
            ManagedSessionRef, SendEventsParams, SendEventsResponse, Session, SessionContext,
        },
        routing::Deployment,
    },
};

pub struct ProviderRequest {
    pub body: Vec<u8>,
    pub headers: HeaderMap,
    pub stream: bool,
}

pub trait Transformation: Send + Sync + 'static {
    fn transform_request(
        &self,
        body: Value,
        deployment: &Deployment,
        inbound_headers: &HeaderMap,
    ) -> Result<ProviderRequest, GatewayError>;

    fn transform_response_headers(&self, upstream: &HeaderMap, stream: bool) -> HeaderMap;
}

#[derive(Clone)]
pub struct Provider {
    pub handler: Arc<dyn Transformation>,
    pub default_api_base: String,
}

#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Provider>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        id: &'static str,
        default_api_base: &'static str,
        handler: impl Transformation,
    ) {
        self.providers.insert(
            id.to_owned(),
            Provider {
                handler: Arc::new(handler),
                default_api_base: default_api_base.to_owned(),
            },
        );
    }

    pub fn get(&self, id: &str) -> Option<Provider> {
        self.providers.get(id).cloned()
    }
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .finish()
    }
}

pub(crate) type AdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentSdkError>> + Send + 'a>>;

pub(crate) trait RuntimeAdapter: Send + Sync {
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

pub(crate) fn adapter(runtime: AgentRuntime) -> Arc<dyn RuntimeAdapter> {
    match runtime {
        AgentRuntime::ClaudeManagedAgents => {
            Arc::new(anthropic::runtime::ClaudeManagedAgentsRuntime)
        }
        AgentRuntime::Cursor => Arc::new(cursor::runtime::CursorRuntime),
    }
}

pub mod model {
    pub use super::{Provider, ProviderRegistry, ProviderRequest, Transformation};
}

pub mod transform {
    pub use super::{Provider, ProviderRegistry, ProviderRequest, Transformation};
}

include!(concat!(env!("OUT_DIR"), "/providers_generated.rs"));
