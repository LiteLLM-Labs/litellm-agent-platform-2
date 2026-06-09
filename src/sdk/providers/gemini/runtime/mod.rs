mod stream;

use futures_util::stream as futures_stream;
use serde_json::{json, Map, Value};

use crate::sdk::agents::{
    response_fields::id, AgentEventStream, AgentRuntime, AgentSdkError, AgentWorkspace,
    CreateAgentParams, CreateEnvironmentParams, CreateSessionParams, DeleteAgentParams,
    DeleteAgentResponse, Environment, GetAgentParams, Lap, ListAgentsParams, ManagedAgent,
    ManagedAgentList, SendEventsParams, SendEventsResponse, Session, SessionContext,
    GEMINI_ANTIGRAVITY,
};
use crate::sdk::providers::base::runtime::{AdapterFuture, RuntimeAdapter};
use stream::{events_from_interaction, list_events_from_interaction};

const BASE_AGENT_ID: &str = "antigravity-preview-05-2026";
const DEFAULT_ENVIRONMENT_ID: &str = "remote";
const SUPPORTED_TOOL_TYPES: &[&str] = &["code_execution", "google_search", "url_context"];

pub(crate) const RUNTIME_ID: &str = GEMINI_ANTIGRAVITY;

pub(crate) struct GeminiAntigravityRuntime;

impl RuntimeAdapter for GeminiAntigravityRuntime {
    fn provider_run_id_from_agent_raw(&self, raw: &Value) -> Option<String> {
        (raw.get("object").and_then(Value::as_str) == Some("interaction"))
            .then(|| raw.get("id").and_then(Value::as_str).map(str::to_owned))
            .flatten()
    }

    fn provider_session_id_from_session_raw(&self, raw: &Value) -> Option<String> {
        raw.get("environment_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn events_from_send_response_raw(&self, raw: &Value) -> Vec<Value> {
        list_events_from_interaction(raw)
    }

    fn create_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateAgentParams,
    ) -> AdapterFuture<'a, ManagedAgent> {
        Box::pin(async move {
            let raw = client
                .post(
                    AgentRuntime::GeminiAntigravity,
                    "/v1beta/agents",
                    &create_agent_body(params)?,
                )
                .await?;
            managed_agent(raw)
        })
    }

    fn list_agents<'a>(
        &'a self,
        client: &'a Lap,
        params: ListAgentsParams,
    ) -> AdapterFuture<'a, ManagedAgentList> {
        Box::pin(async move {
            let raw = client
                .get(AgentRuntime::GeminiAntigravity, &list_agents_path(params))
                .await?;
            let agents = raw
                .get("agents")
                .or_else(|| raw.get("data"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(managed_agent)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ManagedAgentList {
                agents,
                next_page_token: raw
                    .get("next_page_token")
                    .or_else(|| raw.get("nextPageToken"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                raw,
            })
        })
    }

    fn get_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: GetAgentParams,
    ) -> AdapterFuture<'a, ManagedAgent> {
        Box::pin(async move {
            let raw = client
                .get(
                    AgentRuntime::GeminiAntigravity,
                    &format!("/v1beta/agents/{}", params.id),
                )
                .await?;
            managed_agent(raw)
        })
    }

    fn delete_agent<'a>(
        &'a self,
        client: &'a Lap,
        params: DeleteAgentParams,
    ) -> AdapterFuture<'a, DeleteAgentResponse> {
        Box::pin(async move {
            let raw = client
                .delete(
                    AgentRuntime::GeminiAntigravity,
                    &format!("/v1beta/agents/{}", params.id),
                )
                .await?;
            Ok(DeleteAgentResponse { raw })
        })
    }

    fn create_environment<'a>(
        &'a self,
        _client: &'a Lap,
        params: CreateEnvironmentParams,
    ) -> AdapterFuture<'a, Environment> {
        Box::pin(async move {
            let environment = environment_id(params.config);
            let raw = json!({ "id": environment });
            Ok(Environment { id: id(&raw)?, raw })
        })
    }

    fn create_session<'a>(
        &'a self,
        client: &'a Lap,
        params: CreateSessionParams,
    ) -> AdapterFuture<'a, Session> {
        Box::pin(async move {
            if params.agent.trim().is_empty() {
                return Err(AgentSdkError::InvalidRequest(
                    "gemini_antigravity sessions.create requires a non-empty agent id".to_owned(),
                ));
            }
            let environment_id = if params.environment_id.trim().is_empty() {
                DEFAULT_ENVIRONMENT_ID.to_owned()
            } else {
                params.environment_id
            };
            let raw = json!({
                "id": format!("gemini_ses_{}", uuid::Uuid::new_v4().simple()),
                "agent": params.agent,
                "environment_id": environment_id,
                "status": "idle"
            });
            let session = Session {
                id: id(&raw)?,
                agent: raw.get("agent").and_then(Value::as_str).map(str::to_owned),
                environment_id: raw
                    .get("environment_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                status: raw.get("status").and_then(Value::as_str).map(str::to_owned),
                metadata: None,
                created_at: None,
                updated_at: None,
                raw,
            };
            client.remember_session_context(
                &session.id,
                SessionContext::gemini(
                    session.environment_id.clone().unwrap_or_else(|| DEFAULT_ENVIRONMENT_ID.to_owned()),
                    session.agent.clone().unwrap_or_default(),
                    None,
                ),
            )?;
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
            let context = gemini_context(client, session_id)?;
            let raw = client
                .post(
                    AgentRuntime::GeminiAntigravity,
                    "/v1beta/interactions",
                    &interaction_body(&context, &params)?,
                )
                .await?;
            let interaction_id = id(&raw)?;
            client.remember_session_context(
                session_id,
                SessionContext::gemini(
                    context.environment_id,
                    context.agent_id,
                    Some(interaction_id),
                ),
            )?;
            Ok(SendEventsResponse { raw })
        })
    }

    fn stream_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
    ) -> AdapterFuture<'a, AgentEventStream> {
        Box::pin(async move {
            let context = gemini_context(client, session_id)?;
            let Some(interaction_id) = context.interaction_id else {
                return Ok(Box::pin(futures_stream::empty()) as AgentEventStream);
            };
            let raw = client
                .get(
                    AgentRuntime::GeminiAntigravity,
                    &format!("/v1beta/interactions/{interaction_id}"),
                )
                .await?;
            Ok(Box::pin(futures_stream::iter(
                events_from_interaction(&raw).into_iter().map(Ok),
            )) as AgentEventStream)
        })
    }

    fn list_events<'a>(
        &'a self,
        client: &'a Lap,
        session_id: &'a str,
    ) -> AdapterFuture<'a, Value> {
        Box::pin(async move {
            let context = gemini_context(client, session_id)?;
            let Some(interaction_id) = context.interaction_id else {
                return Ok(json!({ "data": [] }));
            };
            let raw = client
                .get(
                    AgentRuntime::GeminiAntigravity,
                    &format!("/v1beta/interactions/{interaction_id}"),
                )
                .await?;
            Ok(json!({ "data": list_events_from_interaction(&raw), "raw": raw }))
        })
    }
}

struct GeminiContext {
    agent_id: String,
    environment_id: String,
    interaction_id: Option<String>,
}

fn create_agent_body(params: CreateAgentParams) -> Result<Value, AgentSdkError> {
    let options = params.lap_provider_options.clone();
    let mut body = Map::new();
    body.insert("id".to_owned(), Value::String(agent_id(&params.name)));
    body.insert("base_agent".to_owned(), Value::String(BASE_AGENT_ID.to_owned()));
    if !params.system.trim().is_empty() {
        body.insert(
            "system_instruction".to_owned(),
            Value::String(params.system),
        );
    }
    if let Some(description) = params.description {
        body.insert("description".to_owned(), Value::String(description));
    }
    let tools = supported_tools(params.tools);
    if !tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(tools));
    }
    let base_environment = base_environment(params.workspace);
    body.insert("base_environment".to_owned(), base_environment);
    if let Some(Value::Object(options)) = options {
        body.extend(options);
    }
    Ok(Value::Object(body))
}

fn agent_id(name: &str) -> String {
    let mut id = String::new();
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !id.is_empty() {
            id.push('-');
            last_was_dash = true;
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    if id.is_empty() {
        "lap-agent".to_owned()
    } else {
        id
    }
}

fn base_environment(workspace: Option<AgentWorkspace>) -> Value {
    let Some(workspace) = workspace else {
        return Value::String(DEFAULT_ENVIRONMENT_ID.to_owned());
    };
    if workspace.repository.trim().is_empty() {
        return Value::String(DEFAULT_ENVIRONMENT_ID.to_owned());
    }
    json!({
        "type": "remote",
        "sources": [{
            "type": "repository",
            "source": workspace.repository,
            "target": "/workspace/repo"
        }]
    })
}

fn supported_tools(tools: Vec<Value>) -> Vec<Value> {
    tools
        .into_iter()
        .filter(|tool| {
            tool.get("type")
                .and_then(Value::as_str)
                .is_some_and(|tool_type| SUPPORTED_TOOL_TYPES.contains(&tool_type))
        })
        .collect()
}

fn managed_agent(raw: Value) -> Result<ManagedAgent, AgentSdkError> {
    Ok(ManagedAgent {
        id: id(&raw)?,
        version: None,
        name: raw
            .get("display_name")
            .or_else(|| raw.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        model: raw
            .get("base_agent")
            .and_then(Value::as_str)
            .map(str::to_owned),
        system: raw
            .get("system_instruction")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tools: raw
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        mcp_servers: Vec::new(),
        metadata: None,
        created_at: None,
        updated_at: None,
        raw,
    })
}

fn environment_id(config: Value) -> String {
    config
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            config
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| DEFAULT_ENVIRONMENT_ID.to_owned())
}

fn interaction_body(
    context: &GeminiContext,
    params: &SendEventsParams,
) -> Result<Value, AgentSdkError> {
    let mut body = Map::new();
    body.insert("agent".to_owned(), Value::String(context.agent_id.clone()));
    body.insert("input".to_owned(), input_from_events(&params.events)?);
    body.insert(
        "environment".to_owned(),
        Value::String(context.environment_id.clone()),
    );
    body.insert("store".to_owned(), Value::Bool(true));
    if let Some(interaction_id) = &context.interaction_id {
        body.insert(
            "previous_interaction_id".to_owned(),
            Value::String(interaction_id.clone()),
        );
    }
    Ok(Value::Object(body))
}

fn input_from_events(events: &[Value]) -> Result<Value, AgentSdkError> {
    let mut parts = Vec::new();
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("user.message") {
            continue;
        }
        match event.get("content") {
            Some(Value::String(text)) => parts.push(json!({ "type": "text", "text": text })),
            Some(Value::Array(content)) => {
                for item in content {
                    if let Some(text) = item.as_str() {
                        parts.push(json!({ "type": "text", "text": text }));
                    } else if item.is_object() {
                        parts.push(item.clone());
                    }
                }
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return Err(AgentSdkError::InvalidRequest(
            "gemini_antigravity requires at least one user.message content block".to_owned(),
        ));
    }
    if parts.len() == 1 {
        if let Some(text) = parts[0].get("text").and_then(Value::as_str) {
            return Ok(Value::String(text.to_owned()));
        }
    }
    Ok(Value::Array(parts))
}

fn gemini_context(client: &Lap, session_id: &str) -> Result<GeminiContext, AgentSdkError> {
    let context = client.context_for_session(session_id)?;
    let agent_id = context
        .as_ref()
        .and_then(|context| context.agent_id.clone())
        .ok_or_else(|| {
            AgentSdkError::InvalidRequest(
                "gemini_antigravity session is missing provider agent id".to_owned(),
            )
        })?;
    Ok(GeminiContext {
        agent_id,
        environment_id: context
            .as_ref()
            .and_then(|context| context.provider_session_id.clone())
            .unwrap_or_else(|| DEFAULT_ENVIRONMENT_ID.to_owned()),
        interaction_id: context.and_then(|context| context.run_id),
    })
}

fn list_agents_path(params: ListAgentsParams) -> String {
    let mut query = Vec::new();
    if let Some(page_size) = params.page_size {
        query.push(format!("pageSize={page_size}"));
    }
    if let Some(page_token) = params.page_token {
        query.push(format!("pageToken={page_token}"));
    }
    if query.is_empty() {
        "/v1beta/agents".to_owned()
    } else {
        format!("/v1beta/agents?{}", query.join("&"))
    }
}
