use serde_json::Value;

use super::{AdapterFuture, RuntimeAdapter};
use crate::sdk::agents::{
    client::Lap,
    response_fields::id,
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, SendEventsParams, SendEventsResponse,
        Session, ANTHROPIC_VERSION, MANAGED_AGENTS_BETA,
    },
};

pub(super) struct ClaudeManagedAgentsRuntime;

impl RuntimeAdapter for ClaudeManagedAgentsRuntime {
    fn configure_request(
        &self,
        request: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        request
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-beta", MANAGED_AGENTS_BETA)
    }

    fn create_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateAgentParams,
    ) -> AdapterFuture<'a, ManagedAgent> {
        Box::pin(async move {
            let raw = client
                .post(
                    AgentRuntime::ClaudeManagedAgents,
                    "/v1/agents",
                    &create_agent_body(params)?,
                )
                .await?;
            Ok(ManagedAgent {
                id: id(&raw)?,
                version: raw.get("version").and_then(Value::as_u64),
                raw,
            })
        })
    }

    fn create_environment<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateEnvironmentParams,
    ) -> AdapterFuture<'a, Environment> {
        Box::pin(async move {
            let raw = client
                .post(
                    AgentRuntime::ClaudeManagedAgents,
                    "/v1/environments",
                    &params,
                )
                .await?;
            Ok(Environment { id: id(&raw)?, raw })
        })
    }

    fn create_session<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateSessionParams,
    ) -> AdapterFuture<'a, Session> {
        Box::pin(async move {
            let raw = client
                .post(AgentRuntime::ClaudeManagedAgents, "/v1/sessions", &params)
                .await?;
            let session = Session { id: id(&raw)?, raw };
            client.remember_session(&session.id, AgentRuntime::ClaudeManagedAgents)?;
            Ok(session)
        })
    }

    fn send_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
        params: SendEventsParams,
    ) -> AdapterFuture<'a, SendEventsResponse> {
        Box::pin(async move {
            let provider_session_id = provider_session_id(client, session_id)?;
            let raw = client
                .post(
                    AgentRuntime::ClaudeManagedAgents,
                    &format!("/v1/sessions/{provider_session_id}/events"),
                    &params,
                )
                .await?;
            Ok(SendEventsResponse { raw })
        })
    }

    fn stream_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
    ) -> AdapterFuture<'a, crate::sdk::agents::events::AgentEventStream> {
        Box::pin(async move {
            let provider_session_id = provider_session_id(client, session_id)?;
            client
                .stream(
                    AgentRuntime::ClaudeManagedAgents,
                    &format!("/v1/sessions/{provider_session_id}/events/stream"),
                )
                .await
        })
    }
}

fn create_agent_body(params: CreateAgentParams) -> Result<Value, AgentSdkError> {
    let options = params.lap_provider_options.clone();
    let mut body = serde_json::to_value(params)?;
    if let Some(Value::Object(options)) = options {
        let Some(body) = body.as_object_mut() else {
            return Ok(body);
        };
        for (key, value) in options {
            body.insert(key, value);
        }
    }
    Ok(body)
}

fn provider_session_id(client: &Lap, session_id: &str) -> Result<String, AgentSdkError> {
    Ok(client
        .context_for_session(session_id)?
        .and_then(|context| context.provider_session_id)
        .unwrap_or_else(|| session_id.to_owned()))
}
