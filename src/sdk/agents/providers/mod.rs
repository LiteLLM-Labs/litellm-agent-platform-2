mod claude_managed_agents;
mod cursor;
pub(crate) mod transform;

use reqwest::Client;

use super::types::LapConfig;
use transform::AgentProviderRegistry;

pub(crate) fn register_all(registry: &mut AgentProviderRegistry, config: &LapConfig, http: Client) {
    claude_managed_agents::init(registry, config, http.clone());
    cursor::init(registry, config, http);
}
