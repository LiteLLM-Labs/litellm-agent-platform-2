use serde::{Deserialize, Serialize};

use crate::proxy::config::GeneralSettings;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilitySettings {
    pub store_spend_logs: bool,
    pub store_prompts_in_spend_logs: bool,
}

impl ObservabilitySettings {
    pub fn from_general_settings(settings: &GeneralSettings) -> Self {
        Self {
            store_spend_logs: !settings.disable_spend_logs,
            store_prompts_in_spend_logs: settings.store_prompts_in_spend_logs,
        }
    }
}
