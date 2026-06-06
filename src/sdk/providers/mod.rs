//! Provider-owned SDK integrations.
//!
//! Each provider folder owns the capabilities it supports, such as `llm/` for
//! gateway request transformation and `runtime/` for managed-agent runtimes.

pub mod llm;
pub mod runtime;

pub mod transform {
    pub use super::llm::{Provider, ProviderRegistry, ProviderRequest, Transformation};
}

include!(concat!(env!("OUT_DIR"), "/providers_generated.rs"));
