use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::{
    events::AgentEventStream,
    providers::{
        register_all,
        transform::{AgentProviderRegistry, ProviderSessionContext},
    },
    types::{
        AgentRuntime, AgentSdkError, CreateAgentParams, CreateEnvironmentParams,
        CreateSessionParams, Environment, LapConfig, ManagedAgent, SendEventsParams,
        SendEventsResponse, Session,
    },
};

#[derive(Clone)]
pub struct Lap {
    inner: Arc<Inner>,
}

struct Inner {
    providers: AgentProviderRegistry,
    session_contexts: Mutex<HashMap<String, ProviderSessionContext>>,
}

impl Lap {
    pub fn new(config: LapConfig) -> Self {
        Self::with_http_client(config, configured_http_client())
    }

    pub(crate) fn with_http_client(config: LapConfig, http: reqwest::Client) -> Self {
        let mut providers = AgentProviderRegistry::new();
        register_all(&mut providers, &config, http);
        Self {
            inner: Arc::new(Inner {
                providers,
                session_contexts: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn beta(&self) -> Beta<'_> {
        Beta { client: self }
    }

    fn default_runtime(&self) -> Result<AgentRuntime, AgentSdkError> {
        if let Some(runtime) = self.inner.providers.only_runtime() {
            Ok(runtime)
        } else if self.inner.providers.is_empty() {
            Err(AgentSdkError::NoRuntimesConfigured)
        } else {
            Err(AgentSdkError::RuntimeRequired)
        }
    }

    fn provider(
        &self,
        runtime: AgentRuntime,
    ) -> Result<super::providers::transform::AgentProvider, AgentSdkError> {
        self.inner
            .providers
            .get(runtime)
            .ok_or(AgentSdkError::RuntimeNotConfigured(runtime))
    }

    fn context_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ProviderSessionContext>, AgentSdkError> {
        let sessions = self
            .inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?;
        Ok(sessions.get(session_id).cloned())
    }

    fn runtime_for_session(&self, session_id: &str) -> Result<AgentRuntime, AgentSdkError> {
        let Some(context) = self.context_for_session(session_id)? else {
            return self.default_runtime();
        };
        Ok(context.runtime)
    }

    fn remember_session(
        &self,
        session_id: &str,
        context: ProviderSessionContext,
    ) -> Result<(), AgentSdkError> {
        self.inner
            .session_contexts
            .lock()
            .map_err(|_| AgentSdkError::StateLock)?
            .insert(session_id.to_owned(), context);
        Ok(())
    }
}

pub struct Beta<'a> {
    client: &'a Lap,
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
        self.client
            .provider(runtime)?
            .handler
            .create_agent(params)
            .await
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
        self.client
            .provider(runtime)?
            .handler
            .create_environment(params)
            .await
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
        let provider_session = self
            .client
            .provider(runtime)?
            .handler
            .create_session(params)
            .await?;
        let session = provider_session.session;
        self.client
            .remember_session(&session.id, provider_session.context)?;
        Ok(session)
    }

    pub fn events(&self) -> SessionEvents<'a> {
        SessionEvents {
            client: self.client,
        }
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
        let context = self.client.context_for_session(session_id)?;
        let sent = self
            .client
            .provider(runtime)?
            .handler
            .send_events(session_id, params, context)
            .await?;
        if let Some(context) = sent.context {
            self.client.remember_session(session_id, context)?;
        }
        Ok(sent.response)
    }

    pub async fn stream(&self, session_id: &str) -> Result<AgentEventStream, AgentSdkError> {
        let runtime = self.client.runtime_for_session(session_id)?;
        let context = self.client.context_for_session(session_id)?;
        self.client
            .provider(runtime)?
            .handler
            .stream_events(session_id, context)
            .await
    }
}

fn configured_http_client() -> reqwest::Client {
    reqwest::Client::new()
}
