use serde_json::{json, Value};

use crate::{db::managed_agents::registry::schema::ManagedAgentRow, errors::GatewayError};

use super::base::{runtime_agent_id, RuntimeCredential, RuntimeProvision, RuntimeSessionInput};

pub async fn provision(
    http: &reqwest::Client,
    agent: &ManagedAgentRow,
    credential: RuntimeCredential,
    input: RuntimeSessionInput,
) -> Result<RuntimeProvision, GatewayError> {
    let source = source(&input.environment)?;
    let target = target(agent, &input)?;
    let body = json!({
        "prompt": { "text": input.prompt },
        "source": source,
        "model": model(agent, &input.environment),
        "target": target
    });
    let url = format!("{}/v0/agents", credential.api_base.trim_end_matches('/'));
    let response = http
        .post(url)
        .bearer_auth(credential.api_key)
        .json(&body)
        .send()
        .await
        .map_err(GatewayError::Upstream)?;
    let status = response.status();
    let payload = response
        .json::<Value>()
        .await
        .map_err(GatewayError::Upstream)?;
    if !status.is_success() {
        return Err(GatewayError::InvalidConfig(format!(
            "cursor launch failed: {payload}"
        )));
    }
    let provider_id = provider_id(agent, &payload);
    Ok(RuntimeProvision {
        runtime_agent_id: provider_id.clone(),
        provider_session_id: None,
        provider_run_id: Some(provider_id),
        provider_url: provider_url(&payload),
        metadata: json!({
            "runtime": "cursor",
            "launch_request": body,
            "launch_response": payload,
        }),
    })
}

fn model<'a>(agent: &'a ManagedAgentRow, environment: &'a Value) -> &'a str {
    environment
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| agent.config.get("model").and_then(Value::as_str))
        .unwrap_or("claude-4-sonnet")
}

fn provider_id(agent: &ManagedAgentRow, payload: &Value) -> String {
    let fallback_id = runtime_agent_id(agent, "cursor");
    payload
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("agent")
                .and_then(|agent| agent.get("id"))
                .and_then(Value::as_str)
        })
        .unwrap_or(&fallback_id)
        .to_owned()
}

fn provider_url(payload: &Value) -> Option<String> {
    payload
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| payload.get("webUrl").and_then(Value::as_str))
        .map(str::to_owned)
}

fn source(environment: &Value) -> Result<Value, GatewayError> {
    let repository = environment
        .get("repository")
        .and_then(Value::as_str)
        .or_else(|| {
            environment
                .get("source")
                .and_then(|source| source.get("repository"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| GatewayError::InvalidJsonMessage("repository is required".to_owned()))?;
    let ref_name = environment
        .get("ref")
        .and_then(Value::as_str)
        .or_else(|| {
            environment
                .get("source")
                .and_then(|source| source.get("ref"))
                .and_then(Value::as_str)
        })
        .unwrap_or("main");
    Ok(json!({ "repository": repository, "ref": ref_name }))
}

fn target(agent: &ManagedAgentRow, input: &RuntimeSessionInput) -> Result<Value, GatewayError> {
    let auto_create_pr = input
        .environment
        .get("auto_create_pr")
        .or_else(|| input.environment.get("autoCreatePr"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let branch = input
        .environment
        .get("target_branch")
        .or_else(|| input.environment.get("branchName"))
        .and_then(Value::as_str)
        .unwrap_or("agent/{agent_id}/{session_id}")
        .replace("{agent_id}", &agent.id)
        .replace("{session_id}", &input.session_id);
    if branch.trim().is_empty() {
        return Err(GatewayError::InvalidJsonMessage(
            "target branch cannot be empty".to_owned(),
        ));
    }
    Ok(json!({
        "autoCreatePr": auto_create_pr,
        "branchName": branch,
    }))
}
