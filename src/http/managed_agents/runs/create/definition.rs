use std::collections::HashMap;

use sqlx::PgPool;

use crate::{
    agents::config::AgentDefinition,
    db::managed_agents::{mcp_servers, registry, skills::compose::compose_agent_system_prompt},
    errors::GatewayError,
};

pub(super) async fn managed_agent_definition(
    pool: &PgPool,
    agent: &registry::schema::ManagedAgentRow,
    gateway_origin: &str,
    master_key: Option<&str>,
) -> Result<AgentDefinition, GatewayError> {
    Ok(AgentDefinition {
        id: Some(agent.id.clone()),
        name: agent.name.clone(),
        description: agent.description.clone(),
        model: agent.model.clone(),
        harness: Some(agent.harness.clone()),
        system: compose_agent_system_prompt(pool, agent).await?,
        mcp_servers: mcp_server_specs(pool, agent, gateway_origin, master_key).await?,
        tools: Vec::<HashMap<String, serde_yaml::Value>>::new(),
        skills: Vec::new(),
    })
}

async fn mcp_server_specs(
    pool: &PgPool,
    agent: &registry::schema::ManagedAgentRow,
    gateway_origin: &str,
    master_key: Option<&str>,
) -> Result<Vec<serde_yaml::Value>, GatewayError> {
    let ids = agent
        .mcp_server_ids
        .as_array()
        .map(|ids| ids.iter().filter_map(serde_json::Value::as_str))
        .into_iter()
        .flatten();
    let mut servers = Vec::new();
    for id in ids {
        if mcp_servers::repository::get(pool, id).await?.is_none() {
            continue;
        }
        let mut headers = serde_json::Map::new();
        if let Some(master_key) = master_key {
            headers.insert(
                "Authorization".to_owned(),
                serde_json::Value::String(format!("Bearer {master_key}")),
            );
        }
        let mut server = serde_json::Map::from_iter([
            ("name".to_owned(), serde_json::Value::String(id.to_owned())),
            (
                "type".to_owned(),
                serde_json::Value::String("http".to_owned()),
            ),
            (
                "url".to_owned(),
                serde_json::Value::String(format!(
                    "{}/mcp/{}",
                    gateway_origin.trim_end_matches('/'),
                    id
                )),
            ),
        ]);
        if !headers.is_empty() {
            server.insert("headers".to_owned(), serde_json::Value::Object(headers));
        }
        servers.push(serde_yaml::to_value(serde_json::Value::Object(server))?);
    }
    Ok(servers)
}
