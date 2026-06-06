use std::convert::Infallible;

use axum::body::Bytes;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    db::managed_agents::sessions::schema::SessionRow,
    errors::GatewayError,
    managed_agents::providers::base::{normalize_runtime, CURSOR_RUNTIME},
    proxy::state::AppState,
    sdk::agents::{
        AgentRuntime, AgentSdkError, Lap, LapConfig, ManagedSessionRef, SendEventsParams,
    },
};

pub(super) async fn runtime_sdk_client(
    state: &AppState,
    runtime: &str,
) -> Result<Lap, GatewayError> {
    let credential = crate::http::agent_runtimes::load_credential(state, runtime).await?;
    let mut config = LapConfig::default();
    match sdk_runtime(runtime)? {
        AgentRuntime::ClaudeManagedAgents => {
            config.anthropic_api_key = Some(credential.api_key);
            config.anthropic_base_url = credential.api_base;
        }
        AgentRuntime::Cursor => {
            config.cursor_api_key = Some(credential.api_key);
            config.cursor_base_url = credential.api_base;
        }
        AgentRuntime::OpenCode => {
            config.opencode_base_url = Some(credential.api_base);
            config.opencode_api_key = Some(credential.api_key.clone());
            config.opencode_password = Some(credential.api_key);
        }
    }
    Ok(Lap::with_http_client(config, state.http.clone()))
}

pub(super) fn register_runtime_session(client: &Lap, row: &SessionRow) -> Result<(), GatewayError> {
    let runtime = row.runtime.as_deref().ok_or_else(|| {
        GatewayError::InvalidConfig("runtime session is missing runtime".to_owned())
    })?;
    let lap_agent_runtime = sdk_runtime(runtime)?;
    let provider_session_id = provider_session_id(row, lap_agent_runtime)?;
    client
        .register_session(ManagedSessionRef {
            session_id: row.id.clone(),
            lap_agent_runtime,
            provider_agent_id: provider_agent_id(lap_agent_runtime, &provider_session_id),
            provider_session_id: Some(provider_session_id),
            provider_run_id: row.provider_run_id.clone(),
        })
        .map_err(agent_sdk_error)
}

pub(super) fn send_events_params(prompt: String) -> SendEventsParams {
    SendEventsParams {
        events: vec![json!({
            "type": "user.message",
            "content": [{ "type": "text", "text": prompt }]
        })],
    }
}

pub(super) fn provider_event_line<T: Serialize>(
    event: Result<T, AgentSdkError>,
) -> Result<Bytes, Infallible> {
    let line = match event {
        Ok(event) => match serde_json::to_string(&event) {
            Ok(payload) => format!("data: {payload}\n\n"),
            Err(error) => error_event_line(error.to_string()),
        },
        Err(error) => error_event_line(error.to_string()),
    };
    Ok(Bytes::from(line))
}

pub(super) fn provider_run_id(runtime: &str, raw: &Value) -> Option<String> {
    if normalize_runtime(runtime) != Some(CURSOR_RUNTIME) {
        return None;
    }
    raw.get("run")
        .and_then(|run| run.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(super) fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::SandboxError(format!(
            "managed agent provider request failed with status {status}: {body}"
        )),
        other => GatewayError::SandboxError(other.to_string()),
    }
}

fn provider_session_id(row: &SessionRow, runtime: AgentRuntime) -> Result<String, GatewayError> {
    match runtime {
        AgentRuntime::ClaudeManagedAgents => row.provider_session_id.clone().ok_or_else(|| {
            GatewayError::InvalidConfig(
                "Claude Agents session is missing provider_session_id".to_owned(),
            )
        }),
        AgentRuntime::Cursor => row.provider_session_id.clone().ok_or_else(|| {
            GatewayError::InvalidConfig("Cursor session is missing provider_session_id".to_owned())
        }),
        AgentRuntime::OpenCode => row.provider_session_id.clone().ok_or_else(|| {
            GatewayError::InvalidConfig(
                "OpenCode session is missing provider_session_id".to_owned(),
            )
        }),
    }
}

fn provider_agent_id(runtime: AgentRuntime, provider_session_id: &str) -> Option<String> {
    match runtime {
        AgentRuntime::Cursor => Some(provider_session_id.to_owned()),
        AgentRuntime::ClaudeManagedAgents | AgentRuntime::OpenCode => None,
    }
}

fn error_event_line(message: String) -> String {
    format!(
        "data: {}\n\n",
        json!({ "type": "session.error", "error": { "message": message } })
    )
}

fn sdk_runtime(runtime: &str) -> Result<AgentRuntime, GatewayError> {
    let Some(runtime) = normalize_runtime(runtime) else {
        return Err(GatewayError::InvalidConfig(format!(
            "unsupported runtime session: {runtime}"
        )));
    };
    AgentRuntime::try_from(runtime)
        .map_err(|_| GatewayError::InvalidConfig(format!("unsupported runtime session: {runtime}")))
}
