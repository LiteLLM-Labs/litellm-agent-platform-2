use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::{json, Value};

use crate::{
    errors::GatewayError,
    http::responses::util::{invalid, item_id},
};

#[derive(Debug)]
pub(super) struct SseEvent {
    pub(super) name: Option<String>,
    pub(super) data: Value,
}

pub(super) fn parse_sse(body: &[u8]) -> Result<Vec<SseEvent>, GatewayError> {
    let raw = std::str::from_utf8(body).map_err(|_| invalid("upstream stream was not utf-8"))?;
    raw.split("\n\n")
        .filter_map(parse_sse_frame)
        .collect::<Result<Vec<_>, _>>()
}

fn parse_sse_frame(frame: &str) -> Option<Result<SseEvent, GatewayError>> {
    let mut name = None;
    let mut data = String::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            append_data_line(&mut data, value);
        }
    }
    if data.is_empty() || data == "[DONE]" {
        None
    } else {
        Some(
            serde_json::from_str(&data)
                .map(|data| SseEvent { name, data })
                .map_err(Into::into),
        )
    }
}

fn append_data_line(data: &mut String, value: &str) {
    if !data.is_empty() {
        data.push('\n');
    }
    data.push_str(value.trim_start());
}

#[derive(Debug)]
pub(super) struct ToolAccumulator {
    pub(super) id: String,
    pub(super) arguments: String,
    call_id: String,
    name: String,
}

impl ToolAccumulator {
    pub(super) fn from_block(block: &Value) -> Self {
        let call_id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("tool_call")
            .to_owned();
        Self {
            id: item_id("fc"),
            call_id,
            name: block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_owned(),
            arguments: String::new(),
        }
    }

    pub(super) fn item(&self, arguments: &str) -> Value {
        json!({
            "id": self.id,
            "type": "function_call",
            "status": "completed",
            "call_id": self.call_id,
            "name": self.name,
            "arguments": arguments
        })
    }
}

pub(super) fn push_sse(output: &mut Vec<u8>, event: &str, data: Value) -> Result<(), GatewayError> {
    output.extend_from_slice(format!("event: {event}\n").as_bytes());
    output.extend_from_slice(b"data: ");
    output.extend_from_slice(&serde_json::to_vec(&data)?);
    output.extend_from_slice(b"\n\n");
    Ok(())
}

pub(super) fn sse_response(status: StatusCode, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
}
