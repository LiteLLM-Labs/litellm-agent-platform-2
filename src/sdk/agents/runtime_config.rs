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
    OpenCode {
        username: String,
        password: Option<String>,
        bearer_token: Option<String>,
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
            RuntimeAuth::OpenCode {
                username,
                password,
                bearer_token,
            } => match password {
                Some(password) => {
                    let encoded =
                        general_purpose::STANDARD.encode(format!("{username}:{password}"));
                    request.header(header::AUTHORIZATION, format!("Basic {encoded}"))
                }
                None => match bearer_token {
                    Some(api_key) => request.bearer_auth(api_key),
                    None => request,
                },
            },
        }
    }

    pub(super) fn authorize_opencode_bearer(
        &self,
        request: RequestBuilder,
    ) -> Option<RequestBuilder> {
        match &self.auth {
            RuntimeAuth::OpenCode {
                bearer_token: Some(api_key),
                ..
            } => Some(request.bearer_auth(api_key)),
            _ => None,
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
                auth: RuntimeAuth::OpenCode {
                    username: config.opencode_username,
                    password: config.opencode_password,
                    bearer_token: config.opencode_api_key,
                },
            },
        );
    }
    if let Some(base_url) = config.hermes_base_url {
        runtimes.insert(
            AgentRuntime::Hermes,
            RuntimeConfig {
                base_url: base_url.trim_end_matches('/').to_owned(),
                auth: RuntimeAuth::Bearer(config.hermes_api_key.unwrap_or_default()),
            },
        );
    }
    runtimes
}

pub(super) fn configured_http_client() -> reqwest::Client {
    reqwest::Client::new()
}
