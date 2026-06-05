use crate::agents::{
    config::AgentDefinition,
    harnesses::{plain_text::PlainTextEvents, shell_quote, HarnessEvents, HarnessRunSpec},
};

pub const ID: &str = "opencode";

pub fn build_run(agent: &AgentDefinition, prompt: &str) -> HarnessRunSpec {
    HarnessRunSpec {
        command: format!(
            "set -euo pipefail\nexport OPENCODE_DISABLE_AUTOUPDATE=1\nif ! command -v opencode >/dev/null 2>&1; then npm install -g opencode-ai@latest >/dev/null 2>&1; fi\nexec opencode run --model {} {}",
            shell_quote(&agent.model),
            shell_quote(prompt),
        ),
        events: HarnessEvents::PlainText(PlainTextEvents),
    }
}
