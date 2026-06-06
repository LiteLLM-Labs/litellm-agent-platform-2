use reqwest::Method;
use serde_json::{json, Value};

use super::{
    client::{Lap, SessionContext},
    cursor,
    events::AgentEventStream,
    response_fields::{id, nested_id, nested_string_field},
    responses::response_json,
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, SendEventsParams, SendEventsResponse,
        Session,
    },
};

pub struct Beta<'a> {
    pub(super) client: &'a Lap,
}

impl<'a> Beta<'a> {
    pub fn agents(&self) -> Agents<'a> {
        Agents {
            client: self.client,
        }
    }

    pub fn environments(&self) -> Environments<'a> {
        Environments {
            client: self.client,
        }
    }

    pub fn sessions(&self) -> Sessions<'a> {
        Sessions {
            client: self.client,
        }
    }
}

pub struct Agents<'a> {
    client: &'a Lap,
}

impl Agents<'_> {
    pub async fn create(&self, params: CreateAgentParams) -> Result<ManagedAgent, AgentSdkError> {
        let runtime = params.lap_agent_runtime;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.create_claude_agent(runtime, params).await,
            AgentRuntime::Cursor => self.create_cursor_agent(runtime, params).await,
        }
    }

    async fn create_claude_agent(
        &self,
        runtime: AgentRuntime,
        params: CreateAgentParams,
    ) -> Result<ManagedAgent, AgentSdkError> {
        let raw = self.client.post(runtime, "/v1/agents", &params).await?;
        Ok(ManagedAgent {
            id: id(&raw)?,
            version: raw.get("version").and_then(Value::as_u64),
            raw,
        })
    }

    async fn create_cursor_agent(
        &self,
        runtime: AgentRuntime,
        params: CreateAgentParams,
    ) -> Result<ManagedAgent, AgentSdkError> {
        let raw = self
            .client
            .post(runtime, "/v1/agents", &cursor::create_agent_body(params))
            .await?;
        let agent_id = nested_id(&raw, "agent")?;
        if let Some(run_id) = cursor::run_id(&raw) {
            self.client.remember_cursor_run(&agent_id, &run_id)?;
        }
        Ok(ManagedAgent {
            id: agent_id,
            version: None,
            raw,
        })
    }
}

pub struct Environments<'a> {
    client: &'a Lap,
}

impl Environments<'_> {
    pub async fn create(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<Environment, AgentSdkError> {
        let runtime = params.lap_agent_runtime;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => {
                let raw = self
                    .client
                    .post(runtime, "/v1/environments", &params)
                    .await?;
                Ok(Environment { id: id(&raw)?, raw })
            }
            AgentRuntime::Cursor => {
                let raw = json!({ "id": params.name });
                Ok(Environment { id: id(&raw)?, raw })
            }
        }
    }
}

pub struct Sessions<'a> {
    client: &'a Lap,
}

impl<'a> Sessions<'a> {
    pub async fn create(&self, params: CreateSessionParams) -> Result<Session, AgentSdkError> {
        let runtime = params
            .lap_agent_runtime
            .map(Ok)
            .unwrap_or_else(|| self.client.default_runtime())?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.create_claude_session(runtime, params).await,
            AgentRuntime::Cursor => self.create_cursor_session(params),
        }
    }

    pub fn events(&self) -> SessionEvents<'a> {
        SessionEvents {
            client: self.client,
        }
    }

    async fn create_claude_session(
        &self,
        runtime: AgentRuntime,
        params: CreateSessionParams,
    ) -> Result<Session, AgentSdkError> {
        let raw = self.client.post(runtime, "/v1/sessions", &params).await?;
        let session = Session { id: id(&raw)?, raw };
        self.client.remember_session(&session.id, runtime)?;
        Ok(session)
    }

    fn create_cursor_session(&self, params: CreateSessionParams) -> Result<Session, AgentSdkError> {
        if params.agent.trim().is_empty() {
            return Err(AgentSdkError::InvalidRequest(
                "cursor sessions.create requires a non-empty Cursor agent id".to_owned(),
            ));
        }
        let raw = json!({ "id": params.agent });
        let session = Session { id: id(&raw)?, raw };
        let run_id = self.client.cursor_run_for_agent(&session.id)?;
        self.client.remember_session_context(
            &session.id,
            SessionContext::cursor(session.id.clone(), run_id),
        )?;
        Ok(session)
    }
}

pub struct SessionEvents<'a> {
    client: &'a Lap,
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
        }
    }

    pub async fn stream(&self, session_id: &str) -> Result<AgentEventStream, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        match runtime {
            AgentRuntime::ClaudeManagedAgents => self.stream_claude_events(session_id).await,
            AgentRuntime::Cursor => self.stream_cursor_events(session_id).await,
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
        let agent_id = self.cursor_agent_id(session_id)?;
        let body = json!({ "prompt": cursor::prompt_from_events(&params.events)? });
        let raw = self
            .client
            .post(
                AgentRuntime::Cursor,
                &format!("/v1/agents/{agent_id}/runs"),
                &body,
            )
            .await?;
        let run_id = nested_string_field(&raw, "run", "id")?;
        self.client
            .remember_session_context(session_id, SessionContext::cursor(agent_id, Some(run_id)))?;
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

    async fn stream_cursor_events(
        &self,
        session_id: &str,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let context = self.client.context_for_session(session_id)?;
        let agent_id = cursor::agent_id_from_context(session_id, context.as_ref());
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

    fn provider_session_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(self
            .client
            .context_for_session(session_id)?
            .and_then(|context| context.provider_session_id)
            .unwrap_or_else(|| session_id.to_owned()))
    }

    fn cursor_agent_id(&self, session_id: &str) -> Result<String, AgentSdkError> {
        Ok(cursor::agent_id_from_context(
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
        cursor::run_id(&raw).ok_or(AgentSdkError::MissingField("latestRunId"))
    }
}
