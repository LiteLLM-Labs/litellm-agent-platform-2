use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::schema::ManagedAgentRow,
        skills::{self, schema::SkillRow},
    },
    errors::GatewayError,
};

/// Compose an agent's downstream system prompt: a catalog of every platform
/// skill, the full bodies of the skills attached to this agent, and the agent's
/// own base system prompt.
///
/// This is the single source of truth for skill → system-prompt composition. It
/// is shared by the non-runtime agent-run path (`runs/create/definition.rs`) and
/// the `claude_managed_agents` runtime session path (`http/sessions/runtime.rs`)
/// so both surfaces send an identical system prompt to the model.
pub async fn compose_agent_system_prompt(
    pool: &PgPool,
    agent: &ManagedAgentRow,
) -> Result<String, GatewayError> {
    let all_skills = skills::repository::list(pool, None).await?;
    let attached_skill_ids = string_array(&agent.skill_ids);
    let attached_skills = all_skills
        .iter()
        .filter(|skill| attached_skill_ids.iter().any(|id| id == &skill.id))
        .collect::<Vec<_>>();
    Ok(compose_agent_system(
        &agent.system,
        &attached_skills,
        &all_skills,
    ))
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

fn compose_agent_system(
    agent_system: &str,
    attached_skills: &[&SkillRow],
    all_skills: &[SkillRow],
) -> String {
    let catalog = all_skills
        .iter()
        .map(skill_catalog_entry)
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

fn skill_catalog_entry(skill: &SkillRow) -> String {
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
}
