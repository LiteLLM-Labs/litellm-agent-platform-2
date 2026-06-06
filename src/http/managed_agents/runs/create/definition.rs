use std::collections::HashMap;

use serde_json::Value;
use sqlx::PgPool;

use crate::{
    agents::config::AgentDefinition,
    db::managed_agents::{registry, skills},
    errors::GatewayError,
};

pub(super) async fn managed_agent_definition(
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

fn skill_catalog_entry(skill: &skills::schema::SkillRow) -> String {
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
