use async_stream::try_stream;
use futures_util::StreamExt;
use serde_json::{json, Value};

use super::events::{AgentEvent, AgentEventStream};

pub(super) fn normalize_opencode_stream(
    session_id: String,
    mut stream: AgentEventStream,
) -> AgentEventStream {
    let stream = try_stream! {
        while let Some(event) = stream.next().await {
            if let Some(event) = normalize_event(&session_id, event?) {
                yield event;
            }
        }
    };
    Box::pin(stream)
}

fn normalize_event(session_id: &str, mut event: AgentEvent) -> Option<AgentEvent> {
    if event_session_id(&event) != Some(session_id) {
        return None;
    }
    if event.event_type == "session.idle" {
        event.event_type = "session.status_idle".to_owned();
        event
            .data
            .entry("stop_reason".to_owned())
            .or_insert_with(|| json!({ "type": "end_turn" }));
    }
    Some(event)
}

fn event_session_id(event: &AgentEvent) -> Option<&str> {
    event
        .data
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .data
                .get("session_id")
                .and_then(Value::as_str)
                .or_else(|| event.data.get("sessionId").and_then(Value::as_str))
        })
}
