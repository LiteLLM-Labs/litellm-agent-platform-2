use serde_json::{json, Value};
use sqlx::PgPool;

use crate::{
    db::managed_agents::{
        registry::{
            self,
            schema::{ManagedAgentRow, UpdateManagedAgent},
        },
        slack,
    },
    errors::GatewayError,
    http::managed_agents::slack::{
        config::{provider_id_for, slack_config},
        types::SlackAgentConfig,
    },
    proxy::state::AppState,
};

use super::{
    factory::{agent_url, FACTORY_RUNTIME},
    public_base_url, required_str,
};

const SLACK_SCOPES: &str = "app_mentions:read,channels:history,channels:read,chat:write,groups:history,groups:read,im:history,im:read,im:write,mpim:history,mpim:read,reactions:write,team:read,users:read";

pub async fn connect_agent_to_slack(
    state: &AppState,
    pool: &PgPool,
    platform_agent_id: &str,
    arguments: Value,
) -> Result<Value, GatewayError> {
    let agent_id = required_str(&arguments, "agent_id")?;
    let channel_id = required_str(&arguments, "channel_id")?;
    let platform = load_agent(pool, platform_agent_id).await?;
    let child = load_agent(pool, agent_id).await?;
    let config = slack_config(&platform)?;
    if is_connected(&config) {
        return connect_existing(
            state,
            pool,
            platform_agent_id,
            child,
            &platform.config,
            &arguments,
        )
        .await;
    }
    create_install(
        state,
        pool,
        platform_agent_id,
        agent_id,
        channel_id,
        &arguments,
    )
    .await
}

pub async fn list_slack_bindings(
    pool: &PgPool,
    platform_agent_id: &str,
) -> Result<Value, GatewayError> {
    Ok(json!({
        "bindings": slack::bindings::list_bindings(pool, platform_agent_id).await?
    }))
}

async fn connect_existing(
    state: &AppState,
    pool: &PgPool,
    platform_agent_id: &str,
    child: ManagedAgentRow,
    platform_config: &Value,
    arguments: &Value,
) -> Result<Value, GatewayError> {
    copy_slack_config(pool, &child, platform_config).await?;
    let binding = slack::bindings::upsert_binding(
        pool,
        platform_agent_id,
        &child.id,
        optional_str(arguments, "team_id"),
        required_str(arguments, "channel_id")?,
        optional_str(arguments, "dm_user_id"),
        optional_str(arguments, "requested_by"),
    )
    .await?;
    Ok(json!({
        "status": "connected",
        "agent_url": agent_url(state, &child.id)?,
        "binding": binding,
        "agent": child
    }))
}

async fn create_install(
    state: &AppState,
    pool: &PgPool,
    platform_agent_id: &str,
    agent_id: &str,
    channel_id: &str,
    arguments: &Value,
) -> Result<Value, GatewayError> {
    let platform = load_agent(pool, platform_agent_id).await?;
    let config = slack_config(&platform)?;
    let client_id = config.client_id.ok_or_else(|| {
        GatewayError::InvalidConfig("slack client_id is not configured".to_owned())
    })?;
    let provider_id = provider_id_for(platform_agent_id);
    let oauth_state =
        slack::repository::create_oauth_state(pool, platform_agent_id, &provider_id).await?;
    let pending = slack::bindings::create_pending_install(
        pool,
        slack::bindings::PendingInstallInput {
            state: &oauth_state,
            platform_agent_id,
            agent_id,
            team_id: optional_str(arguments, "team_id"),
            channel_id,
            dm_user_id: optional_str(arguments, "dm_user_id"),
            requested_by: optional_str(arguments, "requested_by"),
        },
    )
    .await?;
    Ok(json!({
        "status": "install_required",
        "agent_url": agent_url(state, agent_id)?,
        "install_url": install_url(state, &client_id, &provider_id, &oauth_state)?,
        "pending_install": pending
    }))
}

async fn copy_slack_config(
    pool: &PgPool,
    child: &ManagedAgentRow,
    platform_config: &Value,
) -> Result<(), GatewayError> {
    let Some(slack_config) = platform_config
        .get("slack")
        .cloned()
        .filter(Value::is_object)
    else {
        return Err(GatewayError::InvalidConfig(
            "factory agent is missing slack config".to_owned(),
        ));
    };
    registry::repository::update(
        pool,
        &child.id,
        UpdateManagedAgent {
            name: None,
            model: None,
            system: None,
            prompt: None,
            cron: None,
            timezone: None,
            vault_keys: None,
            setup_commands: None,
            max_runtime_minutes: None,
            on_failure: None,
            config: Some(patch_child_slack(&child.config, slack_config)),
            owner_id: None,
            status: None,
            description: None,
            harness: Some(FACTORY_RUNTIME.to_owned()),
            skill_ids: None,
        },
    )
    .await?;
    Ok(())
}

fn install_url(
    state: &AppState,
    client_id: &str,
    provider_id: &str,
    oauth_state: &str,
) -> Result<String, GatewayError> {
    let base_url = public_base_url(state)?;
    let redirect_uri = format!(
        "{}/host-oauth-callback/{provider_id}",
        base_url.trim_end_matches('/')
    );
    Ok(format!(
        "https://slack.com/oauth/v2/authorize?client_id={}&scope={}&redirect_uri={}&state={}",
        encode_component(client_id),
        encode_component(SLACK_SCOPES),
        encode_component(&redirect_uri),
        encode_component(oauth_state)
    ))
}

async fn load_agent(pool: &PgPool, agent_id: &str) -> Result<ManagedAgentRow, GatewayError> {
    registry::repository::get(pool, agent_id)
        .await?
        .ok_or_else(|| GatewayError::UnknownAgent(agent_id.to_owned()))
}

fn is_connected(config: &SlackAgentConfig) -> bool {
    config.status.as_deref() == Some("connected") && config.bot_token_key.is_some()
}

fn patch_child_slack(config: &Value, slack_config: Value) -> Value {
    let mut root = config.as_object().cloned().unwrap_or_default();
    root.insert("runtime".to_owned(), FACTORY_RUNTIME.into());
    root.insert("slack".to_owned(), slack_config);
    Value::Object(root)
}

fn optional_str<'a>(arguments: &'a Value, field: &str) -> Option<&'a str> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn encode_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
