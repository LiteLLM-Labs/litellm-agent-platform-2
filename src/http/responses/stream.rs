use serde_json::{json, Value};

use crate::{
    errors::GatewayError,
    http::responses::{
        request::ResponsesRequest,
        sse::{parse_sse, push_sse, SseEvent, ToolAccumulator},
        util::{item_id, now, response_id},
    },
};

pub(super) use crate::http::responses::sse::sse_response;

pub(super) fn response_stream_body(
    request: &ResponsesRequest,
    body: &[u8],
) -> Result<Vec<u8>, GatewayError> {
    let mut builder = StreamBuilder::new(request);
    builder.push_created()?;
    for event in parse_sse(body)? {
        builder.handle_event(event)?;
    }
    builder.finish()
}

struct StreamBuilder<'a> {
    request: &'a ResponsesRequest,
    output: Vec<u8>,
    response_id: String,
    message_id: String,
    text: String,
    current_tool: Option<ToolAccumulator>,
    usage: Value,
}

impl<'a> StreamBuilder<'a> {
    fn new(request: &'a ResponsesRequest) -> Self {
        Self {
            request,
            output: Vec::new(),
            response_id: response_id(),
            message_id: item_id("msg"),
            text: String::new(),
            current_tool: None,
            usage: json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }),
        }
    }

    fn push_created(&mut self) -> Result<(), GatewayError> {
        self.push(
            "response.created",
            json!({
                "type": "response.created",
                "response": self.response_base("in_progress")
            }),
        )
    }

    fn handle_event(&mut self, event: SseEvent) -> Result<(), GatewayError> {
        match event.name.as_deref() {
            Some("content_block_start") => self.handle_content_start(&event.data),
            Some("content_block_delta") => self.handle_delta(&event.data),
            Some("content_block_stop") => self.handle_content_stop(),
            Some("message_delta") => {
                self.update_usage(&event.data);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_content_start(&mut self, data: &Value) -> Result<(), GatewayError> {
        let Some(block) = data.get("content_block") else {
            return Ok(());
        };
        match block.get("type").and_then(Value::as_str) {
            Some("text") => self.push_message_start(),
            Some("tool_use") => self.push_tool_start(block),
            _ => Ok(()),
        }
    }

    fn push_message_start(&mut self) -> Result<(), GatewayError> {
        self.push(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": message_item(&self.message_id, Vec::new())
            }),
        )?;
        self.push(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": self.message_id,
                "output_index": 0,
                "content_index": 0,
                "part": { "type": "output_text", "text": "", "annotations": [] }
            }),
        )
    }

    fn push_tool_start(&mut self, block: &Value) -> Result<(), GatewayError> {
        let tool = ToolAccumulator::from_block(block);
        self.push(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": tool.item("")
            }),
        )?;
        self.current_tool = Some(tool);
        Ok(())
    }

    fn handle_delta(&mut self, data: &Value) -> Result<(), GatewayError> {
        let Some(delta) = data.get("delta") else {
            return Ok(());
        };
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => self.push_text_delta(delta),
            Some("input_json_delta") => self.push_tool_delta(delta),
            _ => Ok(()),
        }
    }

    fn push_text_delta(&mut self, delta: &Value) -> Result<(), GatewayError> {
        let delta_text = delta.get("text").and_then(Value::as_str).unwrap_or("");
        self.text.push_str(delta_text);
        self.push(
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "item_id": self.message_id,
                "output_index": 0,
                "content_index": 0,
                "delta": delta_text
            }),
        )
    }

    fn push_tool_delta(&mut self, delta: &Value) -> Result<(), GatewayError> {
        let Some(mut tool) = self.current_tool.take() else {
            return Ok(());
        };
        let partial = delta
            .get("partial_json")
            .and_then(Value::as_str)
            .unwrap_or("");
        tool.arguments.push_str(partial);
        self.push(
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": tool.id,
                "output_index": 0,
                "delta": partial
            }),
        )?;
        self.current_tool = Some(tool);
        Ok(())
    }

    fn handle_content_stop(&mut self) -> Result<(), GatewayError> {
        let Some(tool) = self.current_tool.take() else {
            return Ok(());
        };
        self.push_tool_done(&tool)
    }

    fn push_tool_done(&mut self, tool: &ToolAccumulator) -> Result<(), GatewayError> {
        self.push(
            "response.function_call_arguments.done",
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": tool.id,
                "output_index": 0,
                "arguments": tool.arguments
            }),
        )?;
        self.push(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": tool.item(&tool.arguments)
            }),
        )
    }

    fn update_usage(&mut self, data: &Value) {
        if let Some(upstream_usage) = data.get("usage") {
            self.usage = stream_usage(upstream_usage);
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, GatewayError> {
        self.push_text_done()?;
        self.push(
            "response.completed",
            json!({
                "type": "response.completed",
                "response": self.response_base("completed")
            }),
        )?;
        self.output.extend_from_slice(b"data: [DONE]\n\n");
        Ok(self.output)
    }

    fn push_text_done(&mut self) -> Result<(), GatewayError> {
        if self.text.is_empty() {
            return Ok(());
        }
        let part = json!({ "type": "output_text", "text": self.text, "annotations": [] });
        self.push(
            "response.output_text.done",
            text_done(&self.message_id, &part),
        )?;
        self.push(
            "response.content_part.done",
            part_done(&self.message_id, &part),
        )?;
        self.push(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message_item(&self.message_id, vec![part])
            }),
        )
    }

    fn response_base(&self, status: &str) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": now(),
            "status": status,
            "model": self.request.model,
            "output": [],
            "parallel_tool_calls": self.request.parallel_tool_calls.unwrap_or(false),
            "tool_choice": self.request.tool_choice.clone().unwrap_or_else(|| json!("auto")),
            "tools": self.request.tools,
            "usage": self.usage
        })
    }

    fn push(&mut self, event: &str, data: Value) -> Result<(), GatewayError> {
        push_sse(&mut self.output, event, data)
    }
}

fn text_done(message_id: &str, part: &Value) -> Value {
    json!({
        "type": "response.output_text.done",
        "item_id": message_id,
        "output_index": 0,
        "content_index": 0,
        "text": part["text"]
    })
}

fn part_done(message_id: &str, part: &Value) -> Value {
    json!({
        "type": "response.content_part.done",
        "item_id": message_id,
        "output_index": 0,
        "content_index": 0,
        "part": part
    })
}

fn message_item(id: &str, content: Vec<Value>) -> Value {
    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "status": "completed",
        "content": content
    })
}

fn stream_usage(upstream_usage: &Value) -> Value {
    let output_tokens = upstream_usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "input_tokens": 0,
        "output_tokens": output_tokens,
        "total_tokens": output_tokens
    })
}
