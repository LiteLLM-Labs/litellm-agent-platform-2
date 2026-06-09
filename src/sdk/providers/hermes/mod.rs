pub mod runtime;

use crate::sdk::providers::base::runtime::RuntimeAdapterRegistry;

pub fn register_runtime_adapters(registry: &mut RuntimeAdapterRegistry) {
    use crate::sdk::agents::AgentRuntime;
    registry.register(
        AgentRuntime::Hermes,
        runtime::RUNTIME_ID,
        runtime::HermesRuntime,
    );
}
