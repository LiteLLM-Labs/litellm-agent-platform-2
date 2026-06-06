//! Provider-owned SDK integrations.
//!
//! Each provider folder owns the capabilities it supports, such as `llm/` for
//! request translation and `runtime/` for managed-agent runtimes.

pub mod llm {
    pub use crate::sdk::translation::llm::*;
}

pub mod transform {
    pub use crate::sdk::translation::llm::{
        Provider, ProviderRegistry, ProviderRequest, Transformation,
    };
}

include!(concat!(env!("OUT_DIR"), "/providers_generated.rs"));
