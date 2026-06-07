use reqwest::Method;
use serde_json::{json, Value};

use crate::sdk::providers::cursor::runtime::{
    agent_id_from_context as cursor_agent_id_from_context, run_id as cursor_run_id,
};

use super::{
    client::Lap,
    events::AgentEventStream,
    opencode,
    opencode_stream::normalize_opencode_stream,
    responses::response_json,
    types::{AgentRuntime, AgentSdkError, SendEventsParams, SendEventsResponse},
};

pub struct SessionEvents<'a> {
    pub(super) client: &'a Lap,
}

impl SessionEvents<'_> {
    pub async fn send(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.send_claude_events(session_id, params).await,
            AgentRuntime::Cursor => self.send_cursor_events(session_id, params).await,
            AgentRuntime::OpenCode => self.send_opencode_events(session_id, params).await,
        }
    }

    pub async fn stream(&self, session_id: &str) -> Result<AgentEventStream, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.stream_claude_events(session_id).await,
            AgentRuntime::Cursor => self.stream_cursor_events(session_id).await,
            AgentRuntime::OpenCode => self.stream_opencode_events(session_id).await,
        }
    }

    pub async fn list(&self, session_id: &str) -> Result<Value, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.list_claude_events(session_id).await,
            AgentRuntime::Cursor | AgentRuntime::OpenCode => Ok(json!({ "data": [] })),
        }
    }

    async fn send_claude_events(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let provider_session_id = self.provider_session_id(session_id)?;
        let raw = self
            .client
            .post(
                AgentRuntime::ClaudeManagedAgents,
                &format!("/v1/sessions/{provider_session_id}/events"),
                &params,
            )
            .await?;
        Ok(SendEventsResponse { raw })
    }

    async fn send_cursor_events(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        self.client
            .adapter(runtime)?
            .send_events(self.client, session_id, params)
            .await
    }

    async fn send_opencode_events(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let provider_session_id = self.provider_session_id(session_id)?;
        let raw = self
            .client
            .post(
                AgentRuntime::OpenCode,
                &format!("/session/{provider_session_id}/message"),
                &opencode::message_body(&params)?,
            )
            .await?;
        Ok(SendEventsResponse { raw })
    }

    async fn stream_claude_events(
        &self,
        session_id: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let provider_session_id = self.provider_session_id(session_id)?;
        self.client
            .stream(
                AgentRuntime::ClaudeManagedAgents,
                &format!("/v1/sessions/{provider_session_id}/events/stream"),
            )
            .await
    }

    async fn list_claude_events(&self, session_id: &str) -> Result<Value, AgentSdkError> {
        let provider_session_id = self.provider_session_id(session_id)?;
        self.client
            .get(
                AgentRuntime::ClaudeManagedAgents,
                &format!("/v1/sessions/{provider_session_id}/events"),
            )
            .await
    }

    async fn stream_cursor_events(
        &self,
        session_id: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let context = self.client.context_for_session(session_id)?;
        let agent_id = cursor_agent_id_from_context(session_id, context.as_ref());
        let run_id = match context.and_then(|context| context.run_id) {
            Some(run_id) => run_id,
            None => self.latest_cursor_run_id(&agent_id).await?,
        };
        self.client
            .stream(
                AgentRuntime::Cursor,
                &format!("/v1/agents/{agent_id}/runs/{run_id}/stream"),
            )
            .await
    }

    async fn stream_opencode_events(
        &self,
        session_id: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let provider_session_id = self.provider_session_id(session_id)?;
        let stream = self.client.stream(AgentRuntime::OpenCode, "/event").await?;
        Ok(normalize_opencode_stream(provider_session_id, stream))
    }

    fn provider_session_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(self
            .client
            .context_for_session(session_id)?
            .and_then(|context| context.provider_session_id)
            .unwrap_or_else(|| session_id.to_owned()))
    }

    fn cursor_agent_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(cursor_agent_id_from_context(
            session_id,
            self.client.context_for_session(session_id)?.as_ref(),
        ))
    }

    async fn latest_cursor_run_id(&self, agent_id: &str) -> Result<String, AgentSdkError> {
        let response = self
            .client
            .request(
                AgentRuntime::Cursor,
                Method::GET,
                &format!("/v1/agents/{agent_id}"),
            )?
            .send()
            .await?;
        let raw = response_json(response).await?;
        cursor_run_id(&raw).ok_or(AgentSdkError::MissingField("latestRunId"))
    }
}
