use bytes::Bytes;
use serde::Deserialize;

use crate::agents::sandboxes::AgentOutputChunk;

/// Incremental decoder for the OpenSandbox execd command stream.
///
/// execd frames each `ServerStreamEvent` as a JSON object terminated by a blank
/// line (`\n\n`). Some ingress deployments additionally prefix frames with the
/// canonical SSE `data:` field, so both shapes are accepted. JSON string values
/// escape embedded newlines, so a single physical line always holds one event.
#[derive(Default)]
pub(super) struct ExecdEventDecoder {
    buffer: String,
}

impl ExecdEventDecoder {
    pub(super) fn decode(&mut self, bytes: Bytes) -> Vec<AgentOutputChunk> {
        self.buffer.push_str(&String::from_utf8_lossy(&bytes));

        let mut chunks = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let line = self.buffer[..newline].to_owned();
            self.buffer.drain(..=newline);
            if let Some(chunk) = decode_line(&line) {
                chunks.push(chunk);
            }
        }
        chunks
    }
}

fn decode_line(line: &str) -> Option<AgentOutputChunk> {
    let trimmed = line
        .strip_prefix("data:")
        .map(str::trim_start)
        .unwrap_or(line)
        .trim();
    if trimmed.is_empty() {
        return None;
    }

    let event: ServerStreamEvent = serde_json::from_str(trimmed).ok()?;
    let text = event.text?;
    if text.is_empty() {
        return None;
    }

    match event.event_type.as_str() {
        "stdout" | "result" => Some(AgentOutputChunk::stdout(text)),
        "stderr" | "error" => Some(AgentOutputChunk::stderr(text)),
        _ => None,
    }
}

#[derive(Deserialize)]
struct ServerStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::sandboxes::AgentOutputStreamKind;

    #[test]
    fn decodes_split_frames() {
        let mut decoder = ExecdEventDecoder::default();
        let mut chunks = decoder.decode(Bytes::from_static(b"{\"type\":\"stdout\",\"text\":\"hel"));
        assert!(chunks.is_empty());
        chunks.extend(decoder.decode(Bytes::from_static(
            b"lo\\n\"}\n\n{\"type\":\"stderr\",\"text\":\"warn\"}\n\n",
        )));

        assert_eq!(chunks.len(), 2);
        assert!(matches!(chunks[0].stream, AgentOutputStreamKind::Stdout));
        assert_eq!(chunks[0].delta, "hello\n");
        assert!(matches!(chunks[1].stream, AgentOutputStreamKind::Stderr));
        assert_eq!(chunks[1].delta, "warn");
    }

    #[test]
    fn accepts_sse_data_prefix_and_skips_control_events() {
        let mut decoder = ExecdEventDecoder::default();
        let chunks = decoder.decode(Bytes::from_static(
            b"data: {\"type\":\"ping\",\"text\":\"pong\"}\n\ndata: {\"type\":\"stdout\",\"text\":\"ok\"}\n\n",
        ));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].delta, "ok");
    }
}
