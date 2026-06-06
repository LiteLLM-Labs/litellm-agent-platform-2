//! Provider-owned SDK integrations.
//!
//! Each provider folder owns the target endpoints and runtimes it supports.

use std::sync::Arc;

use crate::sdk::{agents::AgentRuntime, transformations::runtime::RuntimeAdapter};

pub use crate::sdk::transformations::base::{
    Provider, ProviderRegistry, ProviderRequest, Transformation,
};

pub(crate) fn adapter(runtime: AgentRuntime) -> Arc<dyn RuntimeAdapter> {
    match runtime {
        AgentRuntime::ClaudeManagedAgents => {
            Arc::new(anthropic::runtime::ClaudeManagedAgentsRuntime)
        }
        AgentRuntime::Cursor => Arc::new(cursor::runtime::CursorRuntime),
    }
}

pub mod model {
    pub use crate::sdk::transformations::base::{
        Provider, ProviderRegistry, ProviderRequest, Transformation,
    };
}

pub mod transform {
    pub use crate::sdk::transformations::base::{
        Provider, ProviderRegistry, ProviderRequest, Transformation,
    };
}

include!(concat!(env!("OUT_DIR"), "/providers_generated.rs"));
