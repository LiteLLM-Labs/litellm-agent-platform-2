use std::collections::HashMap;

use base64::{engine::general_purpose, Engine};
use reqwest::{header, RequestBuilder};

use super::types::{AgentRuntime, LapConfig, ANTHROPIC_VERSION, MANAGED_AGENTS_BETA};

#[derive(Debug, Clone)]
pub(super) struct RuntimeConfig {
    pub(super) base_url: String,
    auth: RuntimeAuth,
}

#[derive(Debug, Clone)]
enum RuntimeAuth {
    AnthropicApiKey(String),
    Bearer(String),
    OpenCodeBasic {
        username: String,
        password: Option<String>,
    },
}

impl RuntimeConfig {
    pub(super) fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.auth {
            RuntimeAuth::AnthropicApiKey(api_key) => request
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("anthropic-beta", MANAGED_AGENTS_BETA),
            RuntimeAuth::Bearer(api_key) => request.bearer_auth(api_key),
            RuntimeAuth::OpenCodeBasic { username, password } => match password {
                Some(password) => {
                    let encoded =
                        general_purpose::STANDARD.encode(format!("{username}:{password}"));
                    request.header(header::AUTHORIZATION, format!("Basic {encoded}"))
                }
                None => request,
            },
        }
    }
}

pub(super) fn runtime_configs(config: LapConfig) -> HashMap<AgentRuntime, RuntimeConfig> {
    let mut runtimes = HashMap::new();
    if let Some(api_key) = config.anthropic_api_key {
        runtimes.insert(
            AgentRuntime::ClaudeManagedAgents,
            RuntimeConfig {
                base_url: config.anthropic_base_url.trim_end_matches('/').to_owned(),
                auth: RuntimeAuth::AnthropicApiKey(api_key),
            },
        );
    }
    if let Some(api_key) = config.cursor_api_key {
        runtimes.insert(
            AgentRuntime::Cursor,
            RuntimeConfig {
                base_url: config.cursor_base_url.trim_end_matches('/').to_owned(),
                auth: RuntimeAuth::Bearer(api_key),
            },
        );
    }
    if let Some(base_url) = config.opencode_base_url {
        runtimes.insert(
            AgentRuntime::OpenCode,
            RuntimeConfig {
                base_url: base_url.trim_end_matches('/').to_owned(),
                auth: RuntimeAuth::OpenCodeBasic {
                    username: config.opencode_username,
                    password: config.opencode_password,
                },
            },
        );
    }
    runtimes
}

pub(super) fn configured_http_client() -> reqwest::Client {
    reqwest::Client::new()
}
