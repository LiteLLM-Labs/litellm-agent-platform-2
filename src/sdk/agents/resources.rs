use serde_json::{json, Value};

use super::{
    client::{Lap, SessionContext},
    cursor, opencode,
    response_fields::{id, nested_id},
    session_events::SessionEvents,
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, ManagedAgent, Session,
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
            AgentRuntime::OpenCode => Err(AgentSdkError::InvalidRequest(
                "agents.create is not supported for opencode".to_owned(),
            )),
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
            AgentRuntime::OpenCode => Err(AgentSdkError::InvalidRequest(
                "environments.create is not supported for opencode".to_owned(),
            )),
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
            AgentRuntime::OpenCode => self.create_opencode_session(params).await,
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

    async fn create_opencode_session(
        &self,
        params: CreateSessionParams,
    ) -> Result<Session, AgentSdkError> {
        let raw = self
            .client
            .post(
                AgentRuntime::OpenCode,
                "/session",
                &opencode::session_body(params.title),
            )
            .await?;
        let session = Session { id: id(&raw)?, raw };
        self.client
            .remember_session(&session.id, AgentRuntime::OpenCode)?;
        Ok(session)
    }
}
