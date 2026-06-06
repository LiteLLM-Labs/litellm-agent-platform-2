//! Proxy-server concerns: config, request auth, shared state. The LLM
//! translation layer (`sdk::llms`) must not depend on anything here.

pub mod auth;
pub mod config;
pub mod credential_crypto;
mod mcp_config;
pub mod provider_credentials;
pub mod state;
