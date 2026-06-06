pub mod transformation;

use reqwest::Client;

use super::transform::{default_if_empty, AgentProviderRegistry, ProviderConfig};
use crate::sdk::agents::{
    types::{AgentRuntime, LapConfig},
    DEFAULT_CURSOR_BASE_URL,
};
use transformation::CursorProvider;

pub fn init(registry: &mut AgentProviderRegistry, config: &LapConfig, http: Client) {
    if let Some(api_key) = config.cursor_api_key.clone() {
        registry.register(
            AgentRuntime::Cursor,
            CursorProvider::new(ProviderConfig::new(
                AgentRuntime::Cursor,
                http,
                api_key,
                default_if_empty(&config.cursor_base_url, DEFAULT_CURSOR_BASE_URL),
            )),
        );
    }
}
