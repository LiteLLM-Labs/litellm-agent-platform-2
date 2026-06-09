use std::convert::Infallible;

use axum::body::Bytes;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    db::managed_agents::sessions::schema::SessionRow,
    errors::GatewayError,
    http::agent_runtimes::{load_credential, RuntimeCredential},
    proxy::state::AppState,
    sdk::{
        agents::{
            AgentRuntime, AgentSdkError, Lap, LapConfig, ManagedSessionRef, SendEventsParams,
        },
        providers,
    },
};

pub(super) async fn runtime_sdk_client(
    state: &AppState,
    runtime: &str,
) -> Result<Lap, GatewayError> {
    let credential = load_credential(state, runtime).await?;
    lap_from_credential(runtime, &credential)
}

pub(super) fn lap_from_credential(
    runtime: &str,
    credential: &RuntimeCredential,
) -> Result<Lap, GatewayError> {
    let sdk_rt = sdk_runtime(runtime)?;
    let mut config = LapConfig::default();
    match sdk_rt {
        AgentRuntime::ClaudeManagedAgents => {
            config.anthropic_api_key = Some(credential.api_key.clone());
            config.anthropic_base_url = credential.api_base.clone();
        }
        AgentRuntime::Cursor => {
            config.cursor_api_key = Some(credential.api_key.clone());
            config.cursor_base_url = credential.api_base.clone();
        }
        AgentRuntime::OpenCode => {
            config.opencode_base_url = Some(credential.api_base.clone());
            config.opencode_api_key = Some(credential.api_key.clone());
            config.opencode_password = Some(credential.api_key.clone());
        }
        AgentRuntime::Hermes => {
            config.hermes_base_url = Some(credential.api_base.clone());
            config.hermes_api_key = Some(credential.api_key.clone());
        }
    }
    Ok(Lap::new(config))
}

pub(super) fn register_runtime_session(client: &Lap, row: &SessionRow) -> Result<(), GatewayError> {
    let runtime = row.runtime.as_deref().ok_or_else(|| {
        GatewayError::InvalidConfig("runtime session is missing runtime".to_owned())
    })?;
    let lap_agent_runtime = sdk_runtime(runtime)?;
    let provider_session_id = row.provider_session_id.clone().ok_or_else(|| {
        GatewayError::InvalidConfig(format!("{runtime} session is missing provider_session_id"))
    })?;
    let provider_agent_id = providers::runtime_registry()
        .entry_for_id(runtime)
        .and_then(|e| {
            e.adapter
                .provider_agent_id_from_session_id(&provider_session_id)
        });
    client
        .register_session(ManagedSessionRef {
            session_id: row.id.clone(),
            lap_agent_runtime,
            provider_agent_id,
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

/// Extract a provider-specific run ID from a `send_events` raw response.
/// Delegates to the adapter — no runtime string literals here.
pub(super) fn provider_run_id(runtime: &str, raw: &Value) -> Option<String> {
    providers::runtime_registry()
        .entry_for_id(runtime)
        .and_then(|e| e.adapter.provider_run_id_from_agent_raw(raw))
}

pub(super) fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::SandboxError(format!(
            "managed agent provider request failed with status {status}: {body}"
        )),
        other => GatewayError::SandboxError(other.to_string()),
    }
}

pub(super) fn sdk_runtime(runtime: &str) -> Result<AgentRuntime, GatewayError> {
    providers::runtime_registry()
        .entry_for_id(runtime)
        .map(|e| e.runtime)
        .ok_or_else(|| GatewayError::InvalidConfig(format!("unsupported runtime: {runtime}")))
}

fn error_event_line(message: String) -> String {
    format!(
        "data: {}\n\n",
        json!({ "type": "session.error", "error": { "message": message } })
    )
}
