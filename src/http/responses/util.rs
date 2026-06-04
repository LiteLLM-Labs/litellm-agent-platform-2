use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use uuid::Uuid;

use crate::errors::GatewayError;

pub(super) fn text_block(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

pub(super) fn message(role: &str, content: Vec<Value>) -> Value {
    json!({ "role": role, "content": content })
}

pub(super) fn merge_adjacent_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for mut next in messages {
        let next_role = next.get("role").and_then(Value::as_str).unwrap_or("user");
        let Some(last) = merged.last_mut() else {
            merged.push(next);
            continue;
        };
        if last.get("role").and_then(Value::as_str) == Some(next_role) {
            append_content(last, &mut next);
        } else {
            merged.push(next);
        }
    }
    merged
}

fn append_content(last: &mut Value, next: &mut Value) {
    if let (Some(last_content), Some(next_content)) = (
        last.get_mut("content").and_then(Value::as_array_mut),
        next.get_mut("content").and_then(Value::as_array_mut),
    ) {
        last_content.append(next_content);
    }
}

pub(super) fn texts_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(texts_from_value).collect(),
        Value::Object(_) => value
            .get("content")
            .map(texts_from_value)
            .unwrap_or_else(|| vec![value_text(value)]),
        _ => vec![value_text(value)],
    }
}

pub(super) fn texts_from_blocks(blocks: &[Value]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

pub(super) fn to_anthropic_role(role: &str) -> &str {
    if role == "assistant" {
        "assistant"
    } else {
        "user"
    }
}

pub(super) fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

pub(super) fn response_id() -> String {
    format!("resp_{}", Uuid::new_v4().simple())
}

pub(super) fn item_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn invalid(message: &str) -> GatewayError {
    GatewayError::InvalidJsonMessage(message.to_owned())
}
