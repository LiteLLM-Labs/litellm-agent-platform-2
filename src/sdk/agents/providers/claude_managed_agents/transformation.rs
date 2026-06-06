use futures_util::future::BoxFuture;
use reqwest::{header, Method};
use serde::Serialize;
use serde_json::Value;

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

pub const CREATE_AGENT_PARAMS: &[&str] = &[
    "name",
    "model",
    "system",
    "description",
    "tools",
    "mcp_servers",
    "metadata",
];
pub const CREATE_ENVIRONMENT_PARAMS: &[&str] =
    &["name", "config", "description", "scope", "metadata"];
pub const CREATE_SESSION_PARAMS: &[&str] = &["agent", "environment_id", "title", "metadata"];
pub const SEND_EVENTS_PARAMS: &[&str] = &["events"];

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
    fn supported_managed_agents_create_agent_params(&self) -> &'static [&'static str] {
        CREATE_AGENT_PARAMS
    }

    fn transform_managed_agents_create_agent_params(
        &self,
        params: CreateAgentParams,
    ) -> Result<Value, AgentSdkError> {
        serde_json::to_value(params).map_err(AgentSdkError::Json)
    }

    fn create_agent<'a>(
        &'a self,
        params: CreateAgentParams,
    ) -> BoxFuture<'a, Result<ManagedAgent, AgentSdkError>> {
        Box::pin(async move {
            let body = self.transform_managed_agents_create_agent_params(params)?;
            let raw = self.post("/v1/agents", &body).await?;
            Ok(ManagedAgent {
                id: string_field(&raw, "id").map_err(|_| AgentSdkError::MissingId)?,
                version: raw.get("version").and_then(serde_json::Value::as_u64),
                raw,
            })
        })
    }

    fn supported_managed_agents_create_environment_params(&self) -> &'static [&'static str] {
        CREATE_ENVIRONMENT_PARAMS
    }

    fn transform_managed_agents_create_environment_params(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<Value, AgentSdkError> {
        serde_json::to_value(params).map_err(AgentSdkError::Json)
    }

    fn create_environment<'a>(
        &'a self,
        params: CreateEnvironmentParams,
    ) -> BoxFuture<'a, Result<Environment, AgentSdkError>> {
        Box::pin(async move {
            let body = self.transform_managed_agents_create_environment_params(params)?;
            let raw = self.post("/v1/environments", &body).await?;
            Ok(Environment {
                id: string_field(&raw, "id").map_err(|_| AgentSdkError::MissingId)?,
                raw,
            })
        })
    }

    fn supported_managed_agents_create_session_params(&self) -> &'static [&'static str] {
        CREATE_SESSION_PARAMS
    }

    fn transform_managed_agents_create_session_params(
        &self,
        params: CreateSessionParams,
    ) -> Result<Value, AgentSdkError> {
        serde_json::to_value(params).map_err(AgentSdkError::Json)
    }

    fn create_session<'a>(
        &'a self,
        params: CreateSessionParams,
    ) -> BoxFuture<'a, Result<ProviderSession, AgentSdkError>> {
        Box::pin(async move {
            let body = self.transform_managed_agents_create_session_params(params)?;
            let raw = self.post("/v1/sessions", &body).await?;
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

    fn supported_managed_agents_send_events_params(&self) -> &'static [&'static str] {
        SEND_EVENTS_PARAMS
    }

    fn transform_managed_agents_send_events_params(
        &self,
        params: SendEventsParams,
    ) -> Result<Value, AgentSdkError> {
        serde_json::to_value(params).map_err(AgentSdkError::Json)
    }

    fn send_events<'a>(
        &'a self,
        session_id: &'a str,
        params: SendEventsParams,
        _context: Option<ProviderSessionContext>,
    ) -> BoxFuture<'a, Result<ProviderSendEventsResponse, AgentSdkError>> {
        Box::pin(async move {
            let body = self.transform_managed_agents_send_events_params(params)?;
            let raw = self
                .post(&format!("/v1/sessions/{session_id}/events"), &body)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::sdk::agents::{
        types::{
            AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
            CreateSessionParams, EnvironmentConfig, EnvironmentNetworking, ManagedAgentMcpServer,
            ManagedAgentMcpServerType, ManagedAgentTool, SendEventsParams, UserEvent,
        },
        DEFAULT_ANTHROPIC_BASE_URL,
    };

    fn provider() -> ClaudeManagedAgentsProvider {
        ClaudeManagedAgentsProvider::new(ProviderConfig::new(
            AgentRuntime::ClaudeManagedAgents,
            reqwest::Client::new(),
            "sk-ant-test".to_owned(),
            DEFAULT_ANTHROPIC_BASE_URL.to_owned(),
        ))
    }

    #[test]
    fn exposes_supported_anthropic_params() {
        let provider = provider();

        assert_eq!(
            provider.supported_managed_agents_create_agent_params(),
            CREATE_AGENT_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_create_environment_params(),
            CREATE_ENVIRONMENT_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_create_session_params(),
            CREATE_SESSION_PARAMS
        );
        assert_eq!(
            provider.supported_managed_agents_send_events_params(),
            SEND_EVENTS_PARAMS
        );
    }

    #[test]
    fn transforms_agent_params_to_anthropic_shape() {
        let mut metadata = HashMap::new();
        metadata.insert("lap_owner".to_owned(), "tests".to_owned());

        let body = provider()
            .transform_managed_agents_create_agent_params(CreateAgentParams {
                lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
                name: "Coding Assistant".to_owned(),
                model: AgentModel::from("claude-opus-4-8"),
                system: "Write clean code.".to_owned(),
                description: Some("Test agent".to_owned()),
                tools: vec![ManagedAgentTool::AgentToolset20260401],
                mcp_servers: vec![ManagedAgentMcpServer {
                    name: "linear".to_owned(),
                    server_type: ManagedAgentMcpServerType::Url,
                    url: "https://mcp.linear.app/sse".to_owned(),
                }],
                metadata: Some(metadata),
            })
            .unwrap();

        assert_eq!(
            body,
            json!({
                "name": "Coding Assistant",
                "model": "claude-opus-4-8",
                "system": "Write clean code.",
                "description": "Test agent",
                "tools": [{ "type": "agent_toolset_20260401" }],
                "mcp_servers": [{
                    "name": "linear",
                    "type": "url",
                    "url": "https://mcp.linear.app/sse"
                }],
                "metadata": { "lap_owner": "tests" }
            })
        );
        assert!(body.get("lap_agent_runtime").is_none());
    }

    #[test]
    fn transforms_environment_session_and_event_params() {
        let provider = provider();

        let environment = provider
            .transform_managed_agents_create_environment_params(CreateEnvironmentParams {
                lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
                name: "quickstart-env".to_owned(),
                config: EnvironmentConfig::Cloud {
                    networking: EnvironmentNetworking::Unrestricted,
                },
                description: None,
                scope: None,
                metadata: None,
            })
            .unwrap();
        let session = provider
            .transform_managed_agents_create_session_params(CreateSessionParams {
                agent: "agent_123".into(),
                environment_id: "env_123".to_owned(),
                title: "Quickstart session".to_owned(),
                lap_agent_runtime: Some(AgentRuntime::ClaudeManagedAgents),
                metadata: None,
            })
            .unwrap();
        let events = provider
            .transform_managed_agents_send_events_params(SendEventsParams {
                events: vec![UserEvent::text("Create fibonacci.txt")],
            })
            .unwrap();

        assert_eq!(
            environment,
            json!({
                "name": "quickstart-env",
                "config": {
                    "type": "cloud",
                    "networking": { "type": "unrestricted" }
                }
            })
        );
        assert_eq!(
            session,
            json!({
                "agent": "agent_123",
                "environment_id": "env_123",
                "title": "Quickstart session"
            })
        );
        assert_eq!(
            events,
            json!({
                "events": [{
                    "type": "user.message",
                    "content": [{ "type": "text", "text": "Create fibonacci.txt" }]
                }]
            })
        );
    }
}
