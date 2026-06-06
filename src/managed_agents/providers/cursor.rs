use serde_json::{json, Map, Value};

use crate::{
    db::managed_agents::registry::schema::ManagedAgentRow,
    errors::GatewayError,
    sdk::agents::{
        AgentModel, AgentRuntime, AgentSdkError, CreateAgentParams, CreateSessionParams, Lap,
        LapConfig,
    },
};

use super::base::{
    RuntimeCredential, RuntimeProvision, RuntimeSessionInput, RuntimeTool, CURSOR_RUNTIME,
};

pub const TOOLS: &[RuntimeTool] = &[];

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let client = Lap::with_http_client(
        LapConfig {
            cursor_api_key: Some(credential.api_key),
            cursor_base_url: credential.api_base,
            ..LapConfig::default()
        },
        http.clone(),
    );
    let provider_agent = client
        .beta()
        .agents()
        .create(CreateAgentParams {
            lap_agent_runtime: AgentRuntime::Cursor,
            lap_provider_options: Some(cursor_provider_options(agent, &input)?),
            name: agent.name.clone(),
            model: AgentModel::from(cursor_model(agent, &input.environment)),
            system: cursor_prompt(agent, &input.prompt, &input.environment),
            description: agent.description.clone(),
            tools: Vec::new(),
            mcp_servers: mcp_servers(agent),
        })
        .await
        .map_err(agent_sdk_error)?;
    let provider_session = client
        .beta()
        .sessions()
        .create(CreateSessionParams {
            agent: provider_agent.id.clone(),
            environment_id: input.session_id.clone(),
            title: format!("{} session", agent.name),
            lap_agent_runtime: Some(AgentRuntime::Cursor),
            metadata: None,
            resources: None,
        })
        .await
        .map_err(agent_sdk_error)?;

    Ok(RuntimeProvision {
        runtime_agent_id: provider_agent.id.clone(),
        provider_session_id: Some(provider_session.id),
        provider_run_id: nested_string(&provider_agent.raw, "run", "id"),
        provider_url: provider_url(&provider_agent.raw),
        metadata: json!({
            "runtime": CURSOR_RUNTIME,
            "agent": provider_agent.raw,
            "session": provider_session.raw,
        }),
    })
}

fn cursor_model(agent: &ManagedAgentRow, environment: &Value) -> String {
    environment
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| agent.config.get("model").and_then(Value::as_str))
        .unwrap_or(&agent.model)
        .to_owned()
}

fn cursor_prompt(agent: &ManagedAgentRow, prompt: &str, environment: &Value) -> String {
    let mut parts = Vec::new();
    if !agent.system.trim().is_empty() {
        parts.push(agent.system.trim().to_owned());
    }
    if let Some(context) = repository_context(environment) {
        parts.push(context);
    }
    if !prompt.trim().is_empty() {
        parts.push(prompt.trim().to_owned());
    }
    parts.join("\n\n")
}

fn mcp_servers(agent: &ManagedAgentRow) -> Vec<Value> {
    let Some(value) = agent
        .config
        .get("mcp_servers")
        .or_else(|| agent.config.get("mcpServers"))
    else {
        return Vec::new();
    };
    if let Some(servers) = value.as_array() {
        return servers.clone();
    }
    value
        .as_object()
        .map(|servers| {
            servers
                .iter()
                .filter_map(|(name, server)| {
                    let mut server = server.as_object()?.clone();
                    server
                        .entry("name".to_owned())
                        .or_insert_with(|| Value::String(name.clone()));
                    Some(Value::Object(server))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cursor_provider_options(
    _agent: &ManagedAgentRow,
    input: &RuntimeSessionInput,
) -> Result<Value, GatewayError> {
    let mut options = Map::new();
    if let Some(repos) = repos(&input.environment)? {
        options.insert("repos".to_owned(), repos);
    }
    let auto_create_pr = input
        .environment
        .get("auto_create_pr")
        .or_else(|| input.environment.get("autoCreatePr"))
        .or_else(|| input.environment.get("autoCreatePR"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    options.insert("autoCreatePR".to_owned(), Value::Bool(auto_create_pr));
    Ok(Value::Object(options))
}

fn repos(environment: &Value) -> Result<Option<Value>, GatewayError> {
    let repository = repository_url(environment);
    let Some(repository) = repository else {
        return Ok(None);
    };
    if repository.trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "repository cannot be empty".to_owned(),
        ));
    }
    Ok(Some(
        json!([{ "url": repository, "startingRef": starting_ref(environment) }]),
    ))
}

fn repository_context(environment: &Value) -> Option<String> {
    let repository = repository_url(environment)?;
    Some(format!(
        "Repository: {repository}\nBase branch: {}",
        starting_ref(environment)
    ))
}

fn repository_url(environment: &Value) -> Option<&str> {
    environment
        .get("repository")
        .and_then(Value::as_str)
        .or_else(|| {
            environment
                .get("source")
                .and_then(|source| source.get("repository"))
                .and_then(Value::as_str)
        })
}

fn starting_ref(environment: &Value) -> &str {
    environment
        .get("ref")
        .and_then(Value::as_str)
        .or_else(|| {
            environment
                .get("source")
                .and_then(|source| source.get("ref"))
                .and_then(Value::as_str)
        })
        .unwrap_or("main")
}

fn provider_url(raw: &Value) -> Option<String> {
    raw.get("url")
        .and_then(Value::as_str)
        .or_else(|| raw.get("webUrl").and_then(Value::as_str))
        .or_else(|| {
            raw.get("agent")
                .and_then(|agent| agent.get("url"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            raw.get("agent")
                .and_then(|agent| agent.get("webUrl"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
}

fn nested_string(raw: &Value, parent: &str, field: &str) -> Option<String> {
    raw.get(parent)
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::InvalidConfig(format!(
            "cursor provider request failed with status {status}: {body}"
        )),
        other => GatewayError::InvalidConfig(other.to_string()),
    }
}
