use std::pin::Pin;

use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::AgentSdkError;

pub type AgentEventStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, AgentSdkError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    event_name: Option<String>,
    data_lines: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<AgentEvent>, AgentSdkError> {
        self.buffer.push_str(std::str::from_utf8(bytes)?);
        let mut events = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let mut line = self.buffer[..index].to_owned();
            self.buffer.drain(..=index);
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.push(event);
            }
        }
        Ok(events)
    }

    pub fn finish(mut self) -> Result<Vec<AgentEvent>, AgentSdkError> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            let event = self.process_line(&line)?;
            if let Some(event) = event {
                return Ok(vec![event]);
            }
        }
        self.flush()
    }

    fn process_line(&mut self, line: &str) -> Result<Option<AgentEvent>, AgentSdkError> {
        if line.is_empty() {
            return self.flush().map(|mut events| events.pop());
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event_name = Some(value.to_owned()),
            "data" => self.data_lines.push(value.to_owned()),
            _ => {}
        }
        Ok(None)
    }

    fn flush(&mut self) -> Result<Vec<AgentEvent>, AgentSdkError> {
        if self.data_lines.is_empty() {
            self.event_name = None;
            return Ok(Vec::new());
        }
        let event = parse_event(self.event_name.take(), self.data_lines.join("\n"))?;
        self.data_lines.clear();
        Ok(vec![event])
    }
}

pub fn parse_sse(input: &str) -> Result<Vec<AgentEvent>, AgentSdkError> {
    let mut parser = SseParser::default();
    let mut events = parser.push(input.as_bytes())?;
    events.extend(parser.finish()?);
    Ok(events)
}

pub(crate) fn stream_events(response: reqwest::Response) -> AgentEventStream {
    let stream = try_stream! {
        let mut parser = SseParser::default();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            for event in parser.push(&chunk?)? {
                yield event;
            }
        }
        for event in parser.finish()? {
            yield event;
        }
    };
    Box::pin(stream)
}

fn parse_event(event_name: Option<String>, payload: String) -> Result<AgentEvent, AgentSdkError> {
    let mut value: Value = serde_json::from_str(&payload)?;
    if let Some(event_name) = event_name {
        if let Some(object) = value.as_object_mut() {
            object
                .entry("type")
                .or_insert_with(|| Value::String(event_name));
        }
    }
    serde_json::from_value(value).map_err(AgentSdkError::Json)
}
