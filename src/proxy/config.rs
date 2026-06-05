use std::{collections::HashMap, fs, path::Path};

use serde::Deserialize;

use crate::{
    agents::config::{validate_agents, AgentDefinition, E2bSandboxParams, OpenSandboxParams},
    errors::GatewayError,
    proxy::mcp_config::{is_mcp_sequence_error, validate_mcp_servers},
};

pub use crate::proxy::mcp_config::{McpAuthType, McpServerEntry, McpTransport};

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub model_list: Vec<ModelEntry>,

    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerEntry>,

    #[serde(default)]
    pub general_settings: GeneralSettings,

    #[serde(default)]
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralSettings {
    pub master_key: Option<String>,
    pub database_url: Option<String>,
    #[serde(default)]
    pub store_prompts_in_spend_logs: bool,
    #[serde(default)]
    pub disable_spend_logs: bool,
    #[serde(default = "default_spend_logs_batch_interval_seconds")]
    pub spend_logs_batch_interval_seconds: u64,
    #[serde(default = "default_spend_logs_batch_size")]
    pub spend_logs_batch_size: usize,
    #[serde(default = "default_spend_logs_queue_capacity")]
    pub spend_logs_queue_capacity: usize,
    pub sandbox_choice: Option<String>,
    #[serde(default)]
    pub e2b_sandbox_params: E2bSandboxParams,
    #[serde(default)]
    pub opensandbox_sandbox_params: OpenSandboxParams,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            master_key: None,
            database_url: None,
            store_prompts_in_spend_logs: false,
            disable_spend_logs: false,
            spend_logs_batch_interval_seconds: default_spend_logs_batch_interval_seconds(),
            spend_logs_batch_size: default_spend_logs_batch_size(),
            spend_logs_queue_capacity: default_spend_logs_queue_capacity(),
            sandbox_choice: None,
            e2b_sandbox_params: E2bSandboxParams::default(),
            opensandbox_sandbox_params: OpenSandboxParams::default(),
        }
    }
}

fn default_spend_logs_batch_interval_seconds() -> u64 {
    10
}

fn default_spend_logs_batch_size() -> usize {
    100
}

fn default_spend_logs_queue_capacity() -> usize {
    10_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub model_name: String,
    pub litellm_params: LiteLlmParams,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiteLlmParams {
    pub model: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,

    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

pub fn load_config(path: &Path) -> Result<GatewayConfig, GatewayError> {
    let raw = fs::read_to_string(path)?;
    let mut config: GatewayConfig = serde_yaml::from_str(&raw).map_err(|error| {
        // `mcp_servers` changed from a list to a dict keyed by server name.
        // serde reports this as an "invalid type: sequence" error; translate it
        // into actionable guidance for anyone upgrading an old config.
        if is_mcp_sequence_error(&raw, &error) {
            GatewayError::InvalidConfig(
                "mcp_servers is now a dict keyed by server name (was a list). \
                 See docs/mcp.md for the new format."
                    .to_owned(),
            )
        } else {
            GatewayError::from(error)
        }
    })?;
    expand_env(&mut config)?;
    validate(&config)?;
    Ok(config)
}

pub fn expand_env_value(value: &str) -> Result<String, GatewayError> {
    let Some(name) = value.strip_prefix("os.environ/") else {
        return Ok(value.to_owned());
    };

    std::env::var(name).map_err(|_| {
        GatewayError::InvalidConfig(format!("environment variable {name} is required"))
    })
}

fn expand_env(config: &mut GatewayConfig) -> Result<(), GatewayError> {
    if let Some(master_key) = config.general_settings.master_key.as_deref() {
        config.general_settings.master_key = Some(expand_env_value(master_key)?);
    }
    if let Some(database_url) = config.general_settings.database_url.as_deref() {
        config.general_settings.database_url = Some(expand_env_value(database_url)?);
    }

    for entry in &mut config.model_list {
        if let Some(api_key) = entry.litellm_params.api_key.as_deref() {
            entry.litellm_params.api_key = Some(expand_env_value(api_key)?);
        }
        if let Some(api_base) = entry.litellm_params.api_base.as_deref() {
            entry.litellm_params.api_base = Some(expand_env_value(api_base)?);
        }
    }

    for server in config.mcp_servers.values_mut() {
        server.url = expand_env_value(&server.url)?;
        if let Some(auth_value) = server.auth_value.as_deref() {
            server.auth_value = Some(expand_env_value(auth_value)?);
        }
        for value in server.static_headers.values_mut() {
            *value = expand_env_value(value)?;
        }
    }

    if let Some(api_key) = config
        .general_settings
        .e2b_sandbox_params
        .e2b_api_key
        .as_deref()
    {
        config.general_settings.e2b_sandbox_params.e2b_api_key = Some(expand_env_value(api_key)?);
    }
    for value in config.general_settings.e2b_sandbox_params.envs.values_mut() {
        *value = expand_env_value(value)?;
    }

    if let Some(api_key) = config
        .general_settings
        .opensandbox_sandbox_params
        .api_key
        .as_deref()
    {
        config.general_settings.opensandbox_sandbox_params.api_key =
            Some(expand_env_value(api_key)?);
    }
    if let Some(token) = config
        .general_settings
        .opensandbox_sandbox_params
        .execd_access_token
        .as_deref()
    {
        config
            .general_settings
            .opensandbox_sandbox_params
            .execd_access_token = Some(expand_env_value(token)?);
    }
    config.general_settings.opensandbox_sandbox_params.api_base =
        expand_env_value(&config.general_settings.opensandbox_sandbox_params.api_base)?;
    for value in config
        .general_settings
        .opensandbox_sandbox_params
        .envs
        .values_mut()
    {
        *value = expand_env_value(value)?;
    }

    Ok(())
}

fn validate(config: &GatewayConfig) -> Result<(), GatewayError> {
    validate_required_surface(config)?;
    validate_model_entries(
        &config.model_list,
        config.general_settings.database_url.is_some(),
    )?;
    validate_mcp_servers(&config.mcp_servers)?;
    validate_agents(
        &config.agents,
        config.general_settings.sandbox_choice.as_deref(),
        &config.general_settings.e2b_sandbox_params,
        &config.general_settings.opensandbox_sandbox_params,
    )?;
    Ok(())
}

fn validate_required_surface(config: &GatewayConfig) -> Result<(), GatewayError> {
    if config.model_list.is_empty()
        && config.mcp_servers.is_empty()
        && config.agents.is_empty()
        && config.general_settings.database_url.is_none()
    {
        return Err(GatewayError::InvalidConfig(
            "model_list, mcp_servers, agents, or general_settings.database_url must contain at least one entry".to_owned(),
        ));
    }
    Ok(())
}

fn validate_model_entries(
    entries: &[ModelEntry],
    has_database_url: bool,
) -> Result<(), GatewayError> {
    for entry in entries {
        if entry.model_name.trim().is_empty() {
            return Err(GatewayError::InvalidConfig(
                "model_name cannot be empty".to_owned(),
            ));
        }

        if !entry.litellm_params.model.contains('/') {
            return Err(GatewayError::InvalidConfig(format!(
                "model must include provider prefix (e.g. anthropic/...), got {}",
                entry.litellm_params.model
            )));
        }

        if entry
            .litellm_params
            .api_key
            .as_deref()
            .unwrap_or("")
            .is_empty()
            && !has_database_url
        {
            return Err(GatewayError::InvalidConfig(format!(
                "{} is missing litellm_params.api_key",
                entry.model_name
            )));
        }
    }
    Ok(())
}
