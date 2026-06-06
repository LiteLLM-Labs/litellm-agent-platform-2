use std::{
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use litellm_rust::sdk::agents::{ANTHROPIC_VERSION, MANAGED_AGENTS_BETA};
use reqwest::header;
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY and creates real Claude Managed Agents resources"]
async fn anthropic_live_raw_stream_compare() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY must be set for the Anthropic live stream compare")?;
    let model = std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_owned());
    let prompt = std::env::var("STREAM_COMPARE_PROMPT")
        .unwrap_or_else(|_| "Reply with exactly: LAP managed agents stream compare ok.".to_owned());
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let client = reqwest::Client::new();

    let agent = client
        .post("https://api.anthropic.com/v1/agents")
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&json!({
            "name": format!("LAP SDK stream compare {suffix}"),
            "model": model,
            "system": "Follow the user's request exactly.",
            "tools": [{ "type": "agent_toolset_20260401" }]
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let agent_id = required_string(&agent, "id")?;
    println!("anthropic resource agent_id={agent_id}");

    let environment = client
        .post("https://api.anthropic.com/v1/environments")
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&json!({
            "name": format!("lap-stream-compare-{suffix}"),
            "config": {
                "type": "cloud",
                "networking": { "type": "unrestricted" }
            }
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let environment_id = required_string(&environment, "id")?;
    println!("anthropic resource environment_id={environment_id}");

    let session = client
        .post("https://api.anthropic.com/v1/sessions")
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&json!({
            "agent": agent_id,
            "environment_id": environment_id,
            "title": "LAP SDK stream compare"
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let session_id = required_string(&session, "id")?;
    println!("anthropic resource session_id={session_id}");

    let response = client
        .get(format!(
            "https://api.anthropic.com/v1/sessions/{session_id}/events/stream"
        ))
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .header(header::ACCEPT, "text/event-stream")
        .send()
        .await?
        .error_for_status()?;

    client
        .post(format!(
            "https://api.anthropic.com/v1/sessions/{session_id}/events"
        ))
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", MANAGED_AGENTS_BETA)
        .json(&json!({
            "events": [{
                "type": "user.message",
                "content": [{
                    "type": "text",
                    "text": prompt
                }]
            }]
        }))
        .send()
        .await?
        .error_for_status()?;

    print_stream_until_terminal("anthropic", response).await
}

#[tokio::test]
#[ignore = "requires CURSOR_API_KEY and creates a real Cursor Cloud Agent"]
async fn cursor_live_raw_stream_compare() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("CURSOR_API_KEY")
        .map_err(|_| "CURSOR_API_KEY must be set for the Cursor live stream compare")?;
    let model = std::env::var("CURSOR_MODEL").unwrap_or_else(|_| "default".to_owned());
    let prompt = std::env::var("STREAM_COMPARE_PROMPT").unwrap_or_else(|_| {
        "Reply with exactly: LAP cursor stream compare ok. Do not modify files.".to_owned()
    });
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let client = reqwest::Client::new();

    let created = client
        .post("https://api.cursor.com/v1/agents")
        .bearer_auth(&api_key)
        .json(&json!({
            "name": format!("LAP SDK stream compare {suffix}"),
            "model": { "id": model },
            "prompt": {
                "text": prompt
            }
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let agent_id = required_nested_string(&created, "agent", "id")?;
    let run_id = required_nested_string(&created, "run", "id")?;
    println!("cursor resource agent_id={agent_id}");
    println!("cursor resource run_id={run_id}");

    let response = client
        .get(format!(
            "https://api.cursor.com/v1/agents/{agent_id}/runs/{run_id}/stream"
        ))
        .bearer_auth(&api_key)
        .header(header::ACCEPT, "text/event-stream")
        .send()
        .await?
        .error_for_status()?;

    let result = print_stream_until_terminal("cursor", response).await;
    let cleanup = client
        .post(format!(
            "https://api.cursor.com/v1/agents/{agent_id}/archive"
        ))
        .bearer_auth(&api_key)
        .send()
        .await;
    if let Err(error) = cleanup {
        eprintln!("cursor cleanup failed for {agent_id}: {error}");
    }
    result
}

async fn print_stream_until_terminal(
    provider: &str,
    response: reqwest::Response,
) -> Result<(), Box<dyn Error>> {
    let mut parser = RawSseParser::default();
    let mut chunks = response.bytes_stream();
    let mut index = 0usize;

    timeout(Duration::from_secs(240), async {
        while let Some(chunk) = chunks.next().await {
            for event in parser.push(&chunk?)? {
                index += 1;
                print_raw_event(provider, index, &event);
                if event.is_terminal() {
                    return Ok(());
                }
            }
        }
        for event in parser.finish() {
            index += 1;
            print_raw_event(provider, index, &event);
            if event.is_terminal() {
                return Ok(());
            }
        }
        Err(format!("{provider} stream ended without terminal event").into())
    })
    .await?
}

fn print_raw_event(provider: &str, index: usize, event: &RawSseEvent) {
    let payload =
        serde_json::from_str::<Value>(&event.data).unwrap_or(Value::String(event.data.clone()));
    let event_type = payload_type(&payload)
        .or(event.event.as_deref())
        .unwrap_or("<none>");
    println!(
        "{provider} #{index} type={event_type} string={}",
        payload_text(&payload).unwrap_or_default(),
    );
}

fn payload_type(payload: &Value) -> Option<&str> {
    payload.get("type").and_then(Value::as_str)
}

fn payload_text(payload: &Value) -> Option<String> {
    for key in ["text", "delta", "token", "result", "message"] {
        if let Some(text) = payload.get(key).and_then(Value::as_str) {
            return Some(format!("{text:?}"));
        }
    }
    let content = payload.get("content")?.as_array()?;
    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(value) = block.get("text").and_then(Value::as_str) {
                text.push_str(value);
            }
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(format!("{text:?}"))
    }
}

fn required_string(value: &Value, field: &'static str) -> Result<String, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing field {field} in {value}").into())
}

fn required_nested_string(
    value: &Value,
    parent: &'static str,
    field: &'static str,
) -> Result<String, Box<dyn Error>> {
    value
        .get(parent)
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing field {parent}.{field} in {value}").into())
}

#[derive(Debug, Default)]
struct RawSseParser {
    buffer: String,
    event: Option<String>,
    data: Vec<String>,
}

impl RawSseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<RawSseEvent>, Box<dyn Error>> {
        self.buffer.push_str(std::str::from_utf8(bytes)?);
        let mut events = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let mut line = self.buffer[..index].to_owned();
            self.buffer.drain(..=index);
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line) {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn finish(mut self) -> Vec<RawSseEvent> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            if let Some(event) = self.process_line(&line) {
                return vec![event];
            }
        }
        self.flush().into_iter().collect()
    }

    fn process_line(&mut self, line: &str) -> Option<RawSseEvent> {
        if line.is_empty() {
            return self.flush();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
        None
    }

    fn flush(&mut self) -> Option<RawSseEvent> {
        if self.data.is_empty() {
            self.event = None;
            return None;
        }
        Some(RawSseEvent {
            event: self.event.take(),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

#[derive(Debug)]
struct RawSseEvent {
    event: Option<String>,
    data: String,
}

impl RawSseEvent {
    fn is_terminal(&self) -> bool {
        let Ok(payload) = serde_json::from_str::<Value>(&self.data) else {
            return false;
        };
        matches!(
            payload.get("type").and_then(Value::as_str),
            Some("session.status_idle" | "session.status_terminated" | "session.error")
        ) || matches!(
            payload.get("status").and_then(Value::as_str),
            Some("FINISHED" | "ERROR" | "CANCELLED" | "EXPIRED")
        ) || self.event.as_deref() == Some("done")
    }
}
