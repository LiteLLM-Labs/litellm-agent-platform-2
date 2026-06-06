use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    agents::{
        config::AgentDefinition,
        events,
        harnesses::{build_harness_run, HarnessEvent, HarnessRunContext},
        runs::AgentRunStatus,
        sandboxes::{SandboxCommand, SandboxRunner},
    },
    db::managed_agents::{
        messages,
        registry::{self, schema::ManagedAgentRow},
        sessions::schema::SessionRow,
    },
    errors::GatewayError,
    managed_agents::providers::base::{CLAUDE_AGENTS_RUNTIME, CURSOR_RUNTIME},
    proxy::state::AppState,
    sdk::agents::SendEventsParams,
};

use super::runtime::{agent_sdk_error, claude_agents_sdk_client};

pub(super) async fn execute_prompt(
    state: Arc<AppState>,
    pool: PgPool,
    row: SessionRow,
    prompt: String,
    model: String,
) -> Result<(), GatewayError> {
    if row.runtime.is_some() {
        return execute_runtime_prompt(state, row, prompt).await;
    }

    let agent = agent_definition(&pool, &state, &row, &model).await?;
    let mut harness_run = build_harness_run(&agent, &prompt)?;
    let context = HarnessRunContext::new(&row.id);
    push_events(&state, &row.id, harness_run.events.start(&context));

    let sandbox = SandboxRunner::from_settings(state.http.clone(), &state.config.general_settings)?;
    let session = sandbox.create(&row.id).await?;
    if let Some(sandbox_id) = session.sandbox_id.clone() {
        state.agent_runs.set_sandbox_id(&row.id, sandbox_id);
    }
    state
        .agent_runs
        .update_status(&row.id, AgentRunStatus::Running);

    let mut assistant_text = String::new();
    let run_result = async {
        let mut stream = sandbox
            .start(
                &session,
                SandboxCommand {
                    command: harness_run.command,
                },
            )
            .await?;
        while let Some(output) = stream.next().await {
            let output = output?;
            if output.delta.is_empty() {
                continue;
            }
            let events = harness_run.events.output(&context, output);
            append_assistant_text(&mut assistant_text, &events);
            push_events(&state, &row.id, events);
        }
        Ok::<(), GatewayError>(())
    }
    .await;

    let _ = sandbox.terminate(&session).await;
    run_result?;
    persist_assistant_message(&pool, &row, &context, &assistant_text).await?;
    state
        .agent_runs
        .update_status(&row.id, AgentRunStatus::Completed);
    push_events(&state, &row.id, harness_run.events.complete(&context));
    Ok(())
}

pub(super) async fn persist_message(
    pool: &PgPool,
    session_id: &str,
    role: &str,
    text: &str,
    finish: Option<&str>,
) -> Result<(), GatewayError> {
    persist_message_with_ids(pool, session_id, role, text, finish, None, None).await
}

async fn execute_runtime_prompt(
    state: Arc<AppState>,
    row: SessionRow,
    prompt: String,
) -> Result<(), GatewayError> {
    match row.runtime.as_deref() {
        Some(CLAUDE_AGENTS_RUNTIME) => execute_claude_agents_prompt(state, row, prompt).await,
        Some(CURSOR_RUNTIME) => Err(GatewayError::InvalidConfig(
            "Cursor runtime sessions are provisioned, but the managed agents SDK does not yet expose Cursor event send/stream".to_owned(),
        )),
        Some(runtime) => Err(GatewayError::InvalidConfig(format!(
            "unsupported runtime session: {runtime}"
        ))),
        None => Err(GatewayError::InvalidConfig(
            "runtime session is missing runtime".to_owned(),
        )),
    }
}

async fn execute_claude_agents_prompt(
    state: Arc<AppState>,
    row: SessionRow,
    prompt: String,
) -> Result<(), GatewayError> {
    let provider_session_id = row.provider_session_id.clone().ok_or_else(|| {
        GatewayError::InvalidConfig(
            "Claude Agents session is missing provider_session_id".to_owned(),
        )
    })?;
    let client = claude_agents_sdk_client(&state).await?;

    state
        .agent_runs
        .update_status(&row.id, AgentRunStatus::Running);
    client
        .beta()
        .sessions()
        .events()
        .send(
            &provider_session_id,
            SendEventsParams {
                events: vec![json!({
                    "type": "user.message",
                    "content": [{ "type": "text", "text": prompt }]
                })],
            },
        )
        .await
        .map_err(agent_sdk_error)?;
    Ok(())
}

async fn agent_definition(
    pool: &PgPool,
    state: &AppState,
    row: &SessionRow,
    model: &str,
) -> Result<AgentDefinition, GatewayError> {
    if let Some(agent_id) = row.agent_id.as_deref() {
        if let Some(agent) = registry::repository::get(pool, agent_id).await? {
            return Ok(managed_agent_definition(agent));
        }
        if let Some(agent) = state
            .config
            .agents
            .iter()
            .find(|agent| agent.id() == agent_id)
        {
            return Ok(agent.clone());
        }
    }

    Ok(AgentDefinition {
        id: Some(row.id.clone()),
        name: row.title.clone(),
        description: None,
        model: model.to_owned(),
        harness: Some(row.harness.clone()),
        system: String::new(),
        mcp_servers: Vec::new(),
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    })
}

fn managed_agent_definition(agent: ManagedAgentRow) -> AgentDefinition {
    AgentDefinition {
        id: Some(agent.id),
        name: agent.name,
        description: agent.description,
        model: agent.model,
        harness: Some(agent.harness),
        system: agent.system,
        mcp_servers: Vec::new(),
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    }
}

fn append_assistant_text(assistant_text: &mut String, events: &[HarnessEvent]) {
    for event in events {
        if event.event == events::MESSAGE_PART_DELTA {
            if let Some(delta) = event.data.get("delta").and_then(Value::as_str) {
                assistant_text.push_str(delta);
            }
        }
    }
}

fn push_events(state: &AppState, session_id: &str, events: Vec<HarnessEvent>) {
    for event in events {
        state
            .agent_runs
            .push_event(session_id, event.event, event.data);
    }
}

async fn persist_assistant_message(
    pool: &PgPool,
    row: &SessionRow,
    context: &HarnessRunContext,
    assistant_text: &str,
) -> Result<(), GatewayError> {
    if assistant_text.is_empty() {
        return Ok(());
    }
    persist_message_with_ids(
        pool,
        &row.id,
        "assistant",
        assistant_text,
        Some("stop"),
        Some(&context.message_id),
        Some(&context.part_id),
    )
    .await
}

async fn persist_message_with_ids(
    pool: &PgPool,
    session_id: &str,
    role: &str,
    text: &str,
    finish: Option<&str>,
    message_id: Option<&str>,
    part_id: Option<&str>,
) -> Result<(), GatewayError> {
    let message_id = message_id
        .map(str::to_owned)
        .unwrap_or_else(|| crate::db::managed_agents::id("msg"));
    let part_id = part_id
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{message_id}_text"));
    let now = crate::db::managed_agents::now_ms();
    let mut info = json!({
        "id": message_id,
        "role": role,
        "sessionID": session_id,
        "time": { "created": now },
    });
    if let Some(finish) = finish {
        info["finish"] = finish.into();
        info["time"]["completed"] = now.into();
    }
    let parts = json!([{
        "id": part_id,
        "messageID": message_id,
        "sessionID": session_id,
        "type": "text",
        "text": text,
    }]);
    messages::repository::append(pool, session_id, &info.to_string(), &parts.to_string()).await?;
    Ok(())
}
