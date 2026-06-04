use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::{json, Value};

use crate::{
    errors::GatewayError,
    http::responses::{
        request::ResponsesRequest,
        util::{invalid, item_id, now, response_id},
    },
};

pub(super) fn create_response_body(
    request: &ResponsesRequest,
    upstream: Value,
) -> Result<Value, GatewayError> {
    let output = response_output(&upstream)?;
    let usage = response_usage(&upstream);
    Ok(json!({
        "id": response_id(),
        "object": "response",
        "created_at": now(),
        "status": "completed",
        "model": request.model,
        "output": output,
        "parallel_tool_calls": request.parallel_tool_calls.unwrap_or(false),
        "tool_choice": request.tool_choice.clone().unwrap_or_else(|| json!("auto")),
        "tools": request.tools,
        "usage": usage,
        "metadata": request.metadata.get("metadata").cloned().unwrap_or(Value::Null)
    }))
}

fn response_output(upstream: &Value) -> Result<Vec<Value>, GatewayError> {
    let content = upstream
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("anthropic response content must be an array"))?;
    let mut output = tool_output_items(content)?;
    let text_parts = text_output_parts(content);
    if !text_parts.is_empty() {
        output.insert(0, message_output_item(text_parts));
    }
    Ok(output)
}

fn text_output_parts(content: &[Value]) -> Vec<Value> {
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(|text| json!({ "type": "output_text", "text": text, "annotations": [] }))
        .collect()
}

fn tool_output_items(content: &[Value]) -> Result<Vec<Value>, GatewayError> {
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(function_call_item)
        .collect()
}

fn message_output_item(content: Vec<Value>) -> Value {
    json!({
        "id": item_id("msg"),
        "type": "message",
        "role": "assistant",
        "status": "completed",
        "content": content
    })
}

pub(super) fn function_call_item(block: &Value) -> Result<Value, GatewayError> {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("anthropic tool_use.name is required"))?;
    let call_id = block.get("id").and_then(Value::as_str).unwrap_or(name);
    Ok(json!({
        "id": item_id("fc"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": name,
        "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({})))?
    }))
}

fn response_usage(upstream: &Value) -> Value {
    let input_tokens = upstream
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = upstream
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens
    })
}

pub(super) fn parse_json(body: &[u8]) -> Result<Value, GatewayError> {
    serde_json::from_slice(body).map_err(GatewayError::InvalidJson)
}

pub(super) fn json_response(status: StatusCode, body: Value) -> Response {
    let mut response = Response::new(Body::from(body.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}
