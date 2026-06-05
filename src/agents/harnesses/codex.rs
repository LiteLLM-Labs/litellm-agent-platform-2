use crate::agents::{
    config::AgentDefinition,
    harnesses::{plain_text::PlainTextEvents, shell_quote, HarnessEvents, HarnessRunSpec},
};

pub const ID: &str = "codex";

pub fn build_run(agent: &AgentDefinition, prompt: &str) -> HarnessRunSpec {
    HarnessRunSpec {
        command: format!(
            "set -euo pipefail\nif ! command -v codex >/dev/null 2>&1; then npm install -g @openai/codex@latest >/dev/null 2>&1; fi\nLITELLM_CODEX_OUT=$(mktemp)\ncodex exec --skip-git-repo-check --model {} --output-last-message \"$LITELLM_CODEX_OUT\" {} >/dev/null 2>&1 || true\ncat \"$LITELLM_CODEX_OUT\"",
            shell_quote(&agent.model),
            shell_quote(prompt),
        ),
        events: HarnessEvents::PlainText(PlainTextEvents),
    }
}
