use serde_json::{json, Value};

use crate::{
    errors::GatewayError,
    http::responses::util::{
        invalid, merge_adjacent_messages, message, text_block, texts_from_blocks, texts_from_value,
        to_anthropic_role, value_text,
    },
};

#[derive(Debug)]
pub(super) struct ResponsesRequest {
    pub model: String,
    pub input: Value,
    pub instructions: Option<Value>,
    pub max_output_tokens: Option<u64>,
    pub stream: bool,
    pub tools: Vec<Value>,
    pub parallel_tool_calls: Option<bool>,
    pub tool_choice: Option<Value>,
    pub metadata: Value,
}

impl ResponsesRequest {
    pub(super) fn from_value(mut body: Value) -> Result<Self, GatewayError> {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .ok_or(GatewayError::MissingModel)?
            .to_owned();
        let input = body
            .get_mut("input")
            .map(Value::take)
            .unwrap_or(Value::Null);
        let instructions = body.get_mut("instructions").map(Value::take);
        let max_output_tokens = body.get("max_output_tokens").and_then(Value::as_u64);
        let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
        let tools = take_array(&mut body, "tools");
        let parallel_tool_calls = body.get("parallel_tool_calls").and_then(Value::as_bool);
        let tool_choice = body.get_mut("tool_choice").map(Value::take);

        Ok(Self {
            model,
            input,
            instructions,
            max_output_tokens,
            stream,
            tools,
            parallel_tool_calls,
            tool_choice,
            metadata: body,
        })
    }

    pub(super) fn to_messages_body(&self) -> Result<Value, GatewayError> {
        let (system, messages) = self.messages()?;
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_output_tokens.unwrap_or(4096),
            "messages": messages,
            "stream": self.stream
        });
        if !system.is_empty() {
            body["system"] = Value::String(system);
        }
        add_tools(&mut body, &self.tools, self.tool_choice.as_ref())?;
        Ok(body)
    }

    fn messages(&self) -> Result<(String, Vec<Value>), GatewayError> {
        let mut system = instruction_text(&self.instructions);
        let mut messages = Vec::new();
        match &self.input {
            Value::String(text) => messages.push(message("user", vec![text_block(text)])),
            Value::Array(items) => push_input_items(items, &mut system, &mut messages)?,
            Value::Null => {}
            _ => return Err(invalid("responses input must be a string or array")),
        }
        if messages.is_empty() {
            messages.push(message("user", vec![text_block("")]));
        }
        Ok((system.join("\n\n"), merge_adjacent_messages(messages)))
    }
}

fn take_array(body: &mut Value, key: &str) -> Vec<Value> {
    body.get_mut(key)
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default()
}

fn instruction_text(instructions: &Option<Value>) -> Vec<String> {
    instructions
        .as_ref()
        .map(texts_from_value)
        .unwrap_or_default()
}

fn push_input_items(
    items: &[Value],
    system: &mut Vec<String>,
    messages: &mut Vec<Value>,
) -> Result<(), GatewayError> {
    for item in items {
        push_input_item(item, system, messages)?;
    }
    Ok(())
}

fn push_input_item(
    item: &Value,
    system: &mut Vec<String>,
    messages: &mut Vec<Value>,
) -> Result<(), GatewayError> {
    match item.get("type").and_then(Value::as_str) {
        Some("message") | None => push_message_item(item, system, messages),
        Some("function_call_output") => push_tool_result(item, messages),
        Some("function_call") => push_tool_use(item, messages),
        Some("item_reference") => Ok(()),
        Some(other) => Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported responses input item type: {other}"
        ))),
    }
}

fn push_message_item(
    item: &Value,
    system: &mut Vec<String>,
    messages: &mut Vec<Value>,
) -> Result<(), GatewayError> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
    let content = content_blocks(item.get("content").unwrap_or(&Value::Null))?;
    if role == "system" || role == "developer" {
        system.extend(texts_from_blocks(&content));
    } else {
        messages.push(message(to_anthropic_role(role), content));
    }
    Ok(())
}

fn push_tool_result(item: &Value, messages: &mut Vec<Value>) -> Result<(), GatewayError> {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("function_call_output.call_id is required"))?;
    let output = item.get("output").unwrap_or(&Value::Null);
    messages.push(message(
        "user",
        vec![json!({
            "type": "tool_result",
            "tool_use_id": call_id,
            "content": value_text(output)
        })],
    ));
    Ok(())
}

fn push_tool_use(item: &Value, messages: &mut Vec<Value>) -> Result<(), GatewayError> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("function_call.name is required"))?;
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(name);
    messages.push(message("assistant", vec![tool_use_block(item, id, name)]));
    Ok(())
}

fn tool_use_block(item: &Value, id: &str, name: &str) -> Value {
    let arguments = item
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .unwrap_or_else(|| json!({}));
    json!({ "type": "tool_use", "id": id, "name": name, "input": arguments })
}

fn content_blocks(content: &Value) -> Result<Vec<Value>, GatewayError> {
    match content {
        Value::String(text) => Ok(vec![text_block(text)]),
        Value::Array(items) => items.iter().map(content_block).collect(),
        Value::Null => Ok(vec![text_block("")]),
        _ => Ok(vec![text_block(&value_text(content))]),
    }
}

fn content_block(item: &Value) -> Result<Value, GatewayError> {
    match item.get("type").and_then(Value::as_str) {
        Some("input_text") | Some("output_text") | Some("text") => Ok(text_block(&value_text(
            item.get("text").unwrap_or(&Value::Null),
        ))),
        Some("input_image") => image_block(item),
        Some("tool_use") | Some("tool_result") => Ok(item.clone()),
        Some(other) => Err(GatewayError::InvalidJsonMessage(format!(
            "unsupported responses content item type: {other}"
        ))),
        None => Ok(text_block(&value_text(item))),
    }
}

fn image_block(item: &Value) -> Result<Value, GatewayError> {
    let image_url = item
        .get("image_url")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("input_image.image_url is required"))?;
    let data = image_url
        .strip_prefix("data:")
        .ok_or_else(|| invalid("only data URL input_image values are supported"))?;
    let (media_type, data) = data
        .split_once(";base64,")
        .ok_or_else(|| invalid("input_image must be a base64 data URL"))?;
    Ok(json!({
        "type": "image",
        "source": { "type": "base64", "media_type": media_type, "data": data }
    }))
}

fn add_tools(
    body: &mut Value,
    tools: &[Value],
    tool_choice: Option<&Value>,
) -> Result<(), GatewayError> {
    let tools = anthropic_tools(tools)?;
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(tool_choice) = anthropic_tool_choice(tool_choice) {
        body["tool_choice"] = tool_choice;
    }
    Ok(())
}

fn anthropic_tools(tools: &[Value]) -> Result<Vec<Value>, GatewayError> {
    let mut converted = Vec::new();
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) == Some("function") {
            converted.push(anthropic_tool(tool)?);
        }
    }
    Ok(converted)
}

fn anthropic_tool(tool: &Value) -> Result<Value, GatewayError> {
    let name = tool
        .get("name")
        .or_else(|| tool.pointer("/function/name"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("function tool name is required"))?;
    let description = tool
        .get("description")
        .or_else(|| tool.pointer("/function/description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let input_schema = tool
        .get("parameters")
        .or_else(|| tool.pointer("/function/parameters"))
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    Ok(json!({ "name": name, "description": description, "input_schema": input_schema }))
}

fn anthropic_tool_choice(tool_choice: Option<&Value>) -> Option<Value> {
    match tool_choice {
        Some(Value::String(choice)) if choice == "auto" => Some(json!({ "type": "auto" })),
        Some(Value::String(choice)) if choice == "required" => Some(json!({ "type": "any" })),
        Some(Value::Object(choice)) => choice
            .get("name")
            .and_then(Value::as_str)
            .map(|name| json!({ "type": "tool", "name": name })),
        _ => None,
    }
}
