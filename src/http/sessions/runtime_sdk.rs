use std::convert::Infallible;

use axum::body::Bytes;
use serde::Serialize;
use serde_json::json;

use crate::{
    db::managed_agents::sessions::schema::SessionRow,
    errors::GatewayError,
    sdk::agents::{
        AgentRuntime, AgentSdkError, Lap, LapConfig, ManagedSessionRef, SendEventsParams,
    },
};

pub(super) fn runtime_sdk_client(
    resolved: &crate::http::runtime_resolution::ResolvedRuntime,
) -> Result<Lap, GatewayError> {
    lap_from_credential(resolved)
}

pub(super) fn lap_from_credential(
    resolved: &crate::http::runtime_resolution::ResolvedRuntime,
) -> Result<Lap, GatewayError> {
    let mut config = LapConfig::default();
    match resolved.agent_runtime {
        AgentRuntime::ClaudeManagedAgents => {
            config.anthropic_api_key = Some(resolved.credential.api_key.clone());
            config.anthropic_base_url = resolved.credential.api_base.clone();
        }
        AgentRuntime::Cursor => {
            config.cursor_api_key = Some(resolved.credential.api_key.clone());
            config.cursor_base_url = resolved.credential.api_base.clone();
        }
        AgentRuntime::OpenCode => {
            config.opencode_base_url = Some(resolved.credential.api_base.clone());
            config.opencode_api_key = Some(resolved.credential.api_key.clone());
            config.opencode_password = Some(resolved.credential.api_key.clone());
        }
    }
    Ok(Lap::new(config))
}

pub(super) fn register_runtime_session(
    client: &Lap,
    row: &SessionRow,
    resolved: &crate::http::runtime_resolution::ResolvedRuntime,
) -> Result<(), GatewayError> {
    let provider_session_id = row.provider_session_id.clone().ok_or_else(|| {
        GatewayError::InvalidConfig(format!(
            "{} session is missing provider_session_id",
            resolved.alias
        ))
    })?;
    let provider_agent_id = resolved
        .adapter
        .provider_agent_id_from_session_id(&provider_session_id);
    client
        .register_session(ManagedSessionRef {
            session_id: row.id.clone(),
            lap_agent_runtime: resolved.agent_runtime,
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

pub(super) fn agent_sdk_error(error: AgentSdkError) -> GatewayError {
    match error {
        AgentSdkError::Provider { status, body } => GatewayError::SandboxError(format!(
            "managed agent provider request failed with status {status}: {body}"
        )),
        other => GatewayError::SandboxError(other.to_string()),
    }
}

fn error_event_line(message: String) -> String {
    format!(
        "data: {}\n\n",
        json!({ "type": "session.error", "error": { "message": message } })
    )
}
