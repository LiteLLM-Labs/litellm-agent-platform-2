use serde_json::{json, Map, Value};

use super::{
    client::SessionContext,
    types::{AgentModel, AgentSdkError, CreateAgentParams},
};

pub(super) fn run_id(raw: &Value) -> Option<String> {
    raw.get("run")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .or_else(|| {
            raw.get("agent")
                .and_then(|value| value.get("latestRunId"))
                .and_then(Value::as_str)
        })
        .or_else(|| raw.get("latestRunId").and_then(Value::as_str))
        .map(str::to_owned)
}

pub(super) fn create_agent_body(params: CreateAgentParams) -> Value {
    let mut body = Map::new();
    body.insert("prompt".to_owned(), json!({ "text": params.system }));
    body.insert("name".to_owned(), Value::String(params.name));
    body.insert("model".to_owned(), model(params.model));
    if let Some(Value::Object(options)) = params.lap_provider_options {
        for (key, value) in options {
            body.insert(key, value);
        }
    }
    if !params.mcp_servers.is_empty() {
        body.insert(
            "mcpServers".to_owned(),
            Value::Array(params.mcp_servers.into_iter().map(mcp_server).collect()),
        );
    }
    Value::Object(body)
}

pub(super) fn prompt_from_events(events: &[Value]) -> Result<Value, AgentSdkError> {
    let mut text = Vec::new();
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("user.message") {
            continue;
        }
        let Some(content) = event.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push(value.to_owned());
                }
            }
        }
    }
    if text.is_empty() {
        return Err(AgentSdkError::InvalidRequest(
            "cursor runtime requires at least one user.message text block".to_owned(),
        ));
    }
    Ok(json!({ "text": text.join("\n\n") }))
}

pub(super) fn agent_id_from_context(session_id: &str, context: Option<&SessionContext>) -> String {
    context
        .and_then(|context| context.agent_id.clone())
        .or_else(|| context.and_then(|context| context.provider_session_id.clone()))
        .unwrap_or_else(|| session_id.to_owned())
}

fn model(model: AgentModel) -> Value {
    match model {
        AgentModel::Id(id) => json!({ "id": id }),
        AgentModel::Config(config) => {
            let mut model = Map::new();
            model.insert("id".to_owned(), Value::String(config.id));
            if let Some(speed) = config.speed {
                model.insert(
                    "params".to_owned(),
                    json!([{ "id": "speed", "value": speed }]),
                );
            }
            Value::Object(model)
        }
    }
}

fn mcp_server(server: Value) -> Value {
    let mut server = match server {
        Value::Object(server) => server,
        _ => Map::new(),
    };
    match server.get("type").and_then(Value::as_str) {
        Some("url") | None => {
            server.insert("type".to_owned(), Value::String("http".to_owned()));
        }
        _ => {}
    }
    Value::Object(server)
}
