use serde_json::json;

use crate::agents::{
    config::AgentDefinition,
    events,
    harnesses::{is_stdout, HarnessEvent, HarnessEvents, HarnessRunContext, HarnessRunSpec},
    sandboxes::AgentOutputChunk,
};

pub const ID: &str = "hello-world";

pub fn build_run(_agent: &AgentDefinition, prompt: &str) -> HarnessRunSpec {
    let greeting = make_greeting(prompt);
    HarnessRunSpec {
        command: format!("printf '%s' {}", shell_quote(&greeting)),
        events: HarnessEvents::HelloWorld(HelloWorldEvents),
    }
}

fn make_greeting(prompt: &str) -> String {
    if looks_like_task_request(prompt) && !looks_like_greeting(prompt) {
        "Hello, World! That sounds like quite a task, but greeting is all I do. \
         Hope your day is going splendidly!"
            .to_owned()
    } else {
        "Hello, World! Wonderful to see you — hope you're having a fantastic day!".to_owned()
    }
}

fn looks_like_greeting(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    lower.contains("hello")
        || lower.contains("hi")
        || lower.contains("hey")
        || lower.contains("howdy")
        || lower.contains("greet")
        || prompt.trim().is_empty()
}

fn looks_like_task_request(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    lower.contains("write")
        || lower.contains("code")
        || lower.contains("implement")
        || lower.contains("build")
        || lower.contains("create")
        || lower.contains("fix")
        || lower.contains("debug")
        || lower.contains("search")
        || lower.contains("find")
        || lower.contains("explain")
        || lower.contains("calculate")
        || lower.contains("analyze")
        || lower.contains("run")
        || lower.contains("execute")
}

#[derive(Debug, Clone)]
pub struct HelloWorldEvents;

impl HelloWorldEvents {
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

    pub fn output(&self, context: &HarnessRunContext, output: AgentOutputChunk) -> Vec<HarnessEvent> {
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
