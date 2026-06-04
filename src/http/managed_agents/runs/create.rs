use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    agents::{
        config::AgentDefinition,
        events,
        harnesses::{build_harness_run, HarnessEvent, HarnessRunContext},
        runs::{event_line, AgentRunStatus},
        sandboxes::{SandboxCommand, SandboxRunner},
    },
    db::managed_agents::{
        registry,
        runs::{repository, schema::CreateRun},
        skills,
    },
    errors::GatewayError,
    http::agents::{has_configured_agent, parse_run_agent_request, start_configured_agent_run},
    proxy::{auth::master_key::require_master_key, state::AppState},
};

use super::types::RunCreateResponse;

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(input): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), GatewayError> {
    require_master_key(
        &headers,
        state.config.general_settings.master_key.as_deref(),
    )?;
    if has_configured_agent(&state, &agent_id) {
        return start_configured_agent_run(state, agent_id, parse_run_agent_request(input)?);
    }

    let Some(pool) = state.db.as_ref() else {
        return Err(GatewayError::MissingDatabase);
    };
    let input: CreateRun = serde_json::from_value(input)?;
    let agent = registry::repository::get(pool, &agent_id)
        .await?
        .ok_or_else(|| GatewayError::NotFound("agent not found".to_owned()))?;
    let prompt = input
        .prompt
        .clone()
        .filter(|prompt| !prompt.trim().is_empty())
        .or_else(|| agent.prompt.clone())
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or_else(|| "Proceed with your task.".to_owned());
    let run = repository::create(pool, &agent_id, agent.session_id.clone(), input).await?;
    state.agent_runs.track_run(&agent_id, &run.id);
    spawn_managed_agent_run(
        state.clone(),
        pool.clone(),
        agent_id.clone(),
        managed_agent_definition(pool, &agent).await?,
        prompt,
        run.id.clone(),
    );
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let logs_url = format!("http://{host}/api/agents/{agent_id}/runs/{}/logs", run.id);
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::to_value(RunCreateResponse {
            run_id: run.id,
            agent_id,
            session_id: run.session_id.unwrap_or_default(),
            status: run.status,
            event_url: "/event".to_owned(),
            logs_url,
        })?),
    ))
}

fn spawn_managed_agent_run(
    state: Arc<AppState>,
    pool: PgPool,
    agent_id: String,
    agent: AgentDefinition,
    prompt: String,
    run_id: String,
) {
    tokio::spawn(async move {
        if let Err(error) =
            execute_managed_agent_run(state.clone(), &pool, &agent_id, agent, prompt, &run_id).await
        {
            let message = error.to_string();
            state.agent_runs.set_error(&run_id, message.clone());
            let _ = repository::fail(&pool, &run_id, &message).await;
            let _ = emit_events(
                &state,
                &pool,
                &agent_id,
                &run_id,
                vec![
                    HarnessEvent::new(
                        events::SESSION_ERROR,
                        json!({ "error": { "message": message } }),
                    ),
                    HarnessEvent::new(events::SESSION_IDLE, json!({ "sessionID": run_id })),
                ],
            )
            .await;
        }
    });
}

async fn execute_managed_agent_run(
    state: Arc<AppState>,
    pool: &PgPool,
    agent_id: &str,
    agent: AgentDefinition,
    prompt: String,
    run_id: &str,
) -> Result<(), GatewayError> {
    let mut harness_run = build_harness_run(&agent, &prompt)?;
    let context = HarnessRunContext::new(run_id);
    emit_events(
        &state,
        pool,
        &agent_id,
        run_id,
        harness_run.events.start(&context),
    )
    .await?;

    let sandbox = SandboxRunner::from_settings(state.http.clone(), &state.config.general_settings)?;
    let session = sandbox.create(run_id).await?;
    if let Some(sandbox_id) = session.sandbox_id.clone() {
        state.agent_runs.set_sandbox_id(run_id, sandbox_id.clone());
        repository::set_running(pool, run_id, Some(&sandbox_id)).await?;
    } else {
        repository::set_running(pool, run_id, None).await?;
    }
    state
        .agent_runs
        .update_status(run_id, AgentRunStatus::Running);

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
            emit_events(&state, pool, &agent_id, run_id, events).await?;
        }
        Ok::<(), GatewayError>(())
    }
    .await;

    let _ = sandbox.terminate(&session).await;
    run_result?;

    state
        .agent_runs
        .update_status(run_id, AgentRunStatus::Completed);
    repository::complete(pool, run_id).await?;
    emit_events(
        &state,
        pool,
        &agent_id,
        run_id,
        harness_run.events.complete(&context),
    )
    .await?;
    Ok(())
}

async fn emit_events(
    state: &AppState,
    pool: &PgPool,
    agent_id: &str,
    run_id: &str,
    events: Vec<HarnessEvent>,
) -> Result<(), GatewayError> {
    for event in events {
        let properties = event_properties(agent_id, run_id, event.data.clone());
        if let Some(line) = event_line(event.event, properties) {
            repository::append_logs(pool, run_id, &line).await?;
        }
        state.agent_runs.push_event(run_id, event.event, event.data);
    }
    Ok(())
}

fn event_properties(agent_id: &str, run_id: &str, mut data: Value) -> Value {
    if let Some(payload) = data.as_object_mut() {
        payload.insert("agent_id".to_owned(), agent_id.to_owned().into());
        payload.insert("run_id".to_owned(), run_id.to_owned().into());
        payload.insert("sessionID".to_owned(), run_id.to_owned().into());
    }
    data
}

async fn managed_agent_definition(
    pool: &PgPool,
    agent: &registry::schema::ManagedAgentRow,
) -> Result<AgentDefinition, GatewayError> {
    let all_skills = skills::repository::list(pool, None).await?;
    let attached_skill_ids = string_array(&agent.skill_ids);
    let attached_skills = all_skills
        .iter()
        .filter(|skill| attached_skill_ids.iter().any(|id| id == &skill.id))
        .collect::<Vec<_>>();
    Ok(AgentDefinition {
        id: Some(agent.id.clone()),
        name: agent.name.clone(),
        description: agent.description.clone(),
        model: agent.model.clone(),
        harness: Some(agent.harness.clone()),
        system: compose_agent_system(&agent.system, &attached_skills, &all_skills),
        mcp_servers: Vec::new(),
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    })
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn compose_agent_system(
    agent_system: &str,
    attached_skills: &[&skills::schema::SkillRow],
    all_skills: &[skills::schema::SkillRow],
) -> String {
    let catalog = all_skills
        .iter()
        .map(|skill| {
            format!(
                "- {} ({}){}",
                skill.name,
                skill.id,
                skill
                    .description
                    .as_ref()
                    .filter(|description| !description.trim().is_empty())
                    .map(|description| format!(": {description}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut parts = vec![format!(
        "## Skills available on this platform\nSkills are reusable capability playbooks. The platform currently has:\n{}",
        if catalog.is_empty() { "(none yet)" } else { &catalog }
    )];
    parts.extend(
        attached_skills
            .iter()
            .map(|skill| format!("## Skill: {}\n{}", skill.name, skill.content)),
    );
    if !agent_system.trim().is_empty() {
        parts.push(agent_system.trim().to_owned());
    }
    parts.join("\n\n---\n\n")
}
