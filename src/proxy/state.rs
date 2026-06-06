use std::sync::Arc;

use reqwest::Client;
use sqlx::PgPool;

use crate::{
    agents::runs::AgentRunStore,
    callbacks::{litellm_db::LiteLLMDBCallback, CallbackManager},
    errors::GatewayError,
    llms::router::Router,
    mcp::registry::McpServerRegistry,
    model_prices::ModelCostMap,
    proxy::{auth::api_keys::GatewayApiKeyStore, config::GatewayConfig},
};

#[derive(Debug)]
pub struct AppState {
    pub config: GatewayConfig,
    pub router: Router,
    pub mcp_servers: McpServerRegistry,
    pub http: Client,
    pub model_cost_map: ModelCostMap,
    pub agent_runs: AgentRunStore,
    pub db: Option<PgPool>,
    pub api_keys: GatewayApiKeyStore,
    pub callbacks: CallbackManager,
}

impl AppState {
    pub fn build_http_client() -> Result<Client, GatewayError> {
        Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .build()
            .map_err(GatewayError::HttpClient)
    }

    pub fn new(
        config: GatewayConfig,
        router: Router,
        http: Client,
        model_cost_map: ModelCostMap,
        db: Option<PgPool>,
    ) -> Result<Self, GatewayError> {
        let callbacks = callbacks(&config, db.clone());
        Ok(Self {
            mcp_servers: McpServerRegistry::from_config(&config)?,
            config,
            router,
            http,
            model_cost_map,
            agent_runs: AgentRunStore::default(),
            db,
            api_keys: GatewayApiKeyStore::default(),
            callbacks,
        })
    }
}

fn callbacks(config: &GatewayConfig, db: Option<PgPool>) -> CallbackManager {
    let Some(pool) = db else {
        return CallbackManager::default();
    };
    if config.general_settings.disable_spend_logs {
        return CallbackManager::default();
    }
    CallbackManager::new(vec![Arc::new(LiteLLMDBCallback::new(
        pool,
        &config.general_settings,
    ))])
}
