use serde_json::{json, Value};

use crate::sdk::agents::{AgentSdkError, SendEventsParams};

pub(super) fn session_body(title: String) -> Value {
    json!({ "title": title })
}

pub(super) fn message_body(params: &SendEventsParams) -> Result<Value, AgentSdkError> {
    Ok(json!({ "parts": parts_from_events(&params.events)? }))
}

fn parts_from_events(events: &[Value]) -> Result<Vec<Value>, AgentSdkError> {
    let mut parts = Vec::new();
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("user.message") {
            continue;
        }
        let Some(content) = event.get("content") else {
            continue;
        };
        if let Some(text) = content.as_str() {
            parts.push(json!({ "type": "text", "text": text }));
            continue;
        }
        if let Some(items) = content.as_array() {
            for item in items {
                if let Some(text) = item.as_str() {
                    parts.push(json!({ "type": "text", "text": text }));
                } else if item.get("type").and_then(Value::as_str) == Some("text") {
                    parts.push(json!({
                        "type": "text",
                        "text": item.get("text").and_then(Value::as_str).unwrap_or_default(),
                    }));
                } else if item.is_object() {
                    parts.push(item.clone());
                }
            }
        }
    }
    if parts.is_empty() {
        return Err(AgentSdkError::InvalidRequest(
            "opencode runtime requires at least one user.message content part".to_owned(),
        ));
    }
    Ok(parts)
}
