pub mod transformation;

use reqwest::Client;

use super::transform::{default_if_empty, AgentProviderRegistry, ProviderConfig};
use crate::sdk::agents::{
    types::{AgentRuntime, LapConfig},
    DEFAULT_ANTHROPIC_BASE_URL,
};
use transformation::ClaudeManagedAgentsProvider;

pub fn init(registry: &mut AgentProviderRegistry, config: &LapConfig, http: Client) {
    if let Some(api_key) = config.anthropic_api_key.clone() {
        registry.register(
            AgentRuntime::ClaudeManagedAgents,
            ClaudeManagedAgentsProvider::new(ProviderConfig::new(
                AgentRuntime::ClaudeManagedAgents,
                http,
                api_key,
                default_if_empty(&config.anthropic_base_url, DEFAULT_ANTHROPIC_BASE_URL),
            )),
        );
    }
}
