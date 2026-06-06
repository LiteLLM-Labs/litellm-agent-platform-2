use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::schema::ManagedAgentRow,
        skills::{self, schema::SkillRow},
    },
    errors::GatewayError,
};

/// Compose an agent's downstream system prompt: the full bodies of the skills
/// **attached to this agent** (by `skill_ids`) followed by the agent's own base
/// system prompt. Skills the agent has not attached are never included — the
/// system prompt must not enumerate other agents' skills.
///
/// This is the single source of truth for skill → system-prompt composition. It
/// is shared by the non-runtime agent-run path (`runs/create/definition.rs`) and
/// the `claude_managed_agents` runtime session path (`http/sessions/runtime.rs`)
/// so both surfaces send an identical system prompt to the model.
pub async fn compose_agent_system_prompt(
    pool: &PgPool,
    agent: &ManagedAgentRow,
) -> Result<String, GatewayError> {
    let attached_skill_ids = string_array(&agent.skill_ids);
    if attached_skill_ids.is_empty() {
        return Ok(agent.system.trim().to_owned());
    }
    let all_skills = skills::repository::list(pool, None).await?;
    let attached_skills = all_skills
        .iter()
        .filter(|skill| attached_skill_ids.iter().any(|id| id == &skill.id))
        .collect::<Vec<_>>();
    Ok(compose_agent_system(&agent.system, &attached_skills))
}

/// Extract a JSON array of strings (the agent's `skill_ids`) into a `Vec<String>`.
pub fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn compose_agent_system(agent_system: &str, attached_skills: &[&SkillRow]) -> String {
    let mut parts = attached_skills
        .iter()
        .map(|skill| format!("## Skill: {}\n{}", skill.name, skill.content))
        .collect::<Vec<_>>();
    if !agent_system.trim().is_empty() {
        parts.push(agent_system.trim().to_owned());
    }
    parts.join("\n\n---\n\n")
}
