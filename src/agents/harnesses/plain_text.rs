use serde_json::json;

use crate::agents::{
    events,
    harnesses::{is_stdout, HarnessEvent, HarnessRunContext},
    sandboxes::AgentOutputChunk,
};

/// Event mapping for harnesses that print the assistant reply as plain text on
/// stdout (e.g. `opencode run`, `codex exec --output-last-message`).
///
/// stdout deltas are forwarded verbatim as `message.part.delta` text so the
/// liteharness event stream looks identical regardless of which CLI produced
/// the answer. stderr is treated as diagnostic noise and dropped.
#[derive(Debug, Clone, Default)]
pub struct PlainTextEvents;

impl PlainTextEvents {
    pub fn start(&self, context: &HarnessRunContext) -> Vec<HarnessEvent> {
        vec![
            HarnessEvent::new(
                events::SESSION_STATUS,
                json!({ "status": { "type": "busy" } }),
            ),
            HarnessEvent::new(
                events::MESSAGE_UPDATED,
                json!({
                    "info": {
                        "id": context.message_id,
                        "role": "assistant",
                        "sessionID": context.run_id,
                    }
                }),
            ),
            HarnessEvent::new(
                events::MESSAGE_PART_UPDATED,
                json!({
                    "part": {
                        "id": context.part_id,
                        "messageID": context.message_id,
                        "sessionID": context.run_id,
                        "type": "text",
                        "text": "",
                    }
                }),
            ),
        ]
    }

    pub fn output(
        &mut self,
        context: &HarnessRunContext,
        output: AgentOutputChunk,
    ) -> Vec<HarnessEvent> {
        if output.delta.is_empty() || !is_stdout(output.stream) {
            return Vec::new();
        }
        vec![HarnessEvent::new(
            events::MESSAGE_PART_DELTA,
            json!({
                "messageID": context.message_id,
                "partID": context.part_id,
                "field": "text",
                "delta": output.delta,
            }),
        )]
    }

    pub fn complete(&self, context: &HarnessRunContext) -> Vec<HarnessEvent> {
        vec![
            HarnessEvent::new(
                events::MESSAGE_UPDATED,
                json!({
                    "info": {
                        "id": context.message_id,
                        "role": "assistant",
                        "finish": "stop",
                        "sessionID": context.run_id,
                    }
                }),
            ),
            HarnessEvent::new(events::SESSION_IDLE, json!({ "sessionID": context.run_id })),
        ]
    }
}
