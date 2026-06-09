/**
 * Event translation: hermes stdout → Anthropic Managed Agents API SSE events.
 *
 * The SDK's SseParser reads `event: <type>\ndata: <json>\n\n` blocks.
 * AgentEvent is deserialized with #[serde(flatten)], so data must be flat JSON —
 * NOT wrapped under a "properties" key.
 *
 * Hermes outputs plain text to stdout, so every non-empty chunk becomes an
 * agent.message text delta. Lifecycle events (idle/error) are emitted by
 * index.mjs at the end of a turn.
 */

/**
 * Build an SSE frame understood by the Anthropic SDK parser.
 * @param {string} event  - event type (e.g. "agent.message")
 * @param {object} data   - flat JSON payload
 * @returns {string}
 */
export function sseFrame(event, data) {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
}

/**
 * Map a chunk of hermes stdout to an agent.message SSE frame.
 * Returns null for empty chunks.
 */
export function textDeltaFrame(text, model) {
  if (!text) return null;
  return sseFrame("agent.message", {
    content: [{ type: "text", text }],
    model: model || null,
    stop_reason: null,
  });
}

export function idleFrame() {
  return sseFrame("session.status_idle", {
    stop_reason: { type: "end_turn" },
  });
}

export function errorFrame(message) {
  return sseFrame("session.error", {
    error: { message: String(message) },
  });
}

/**
 * Map an Anthropic user.message events array to plain text for the hermes CLI.
 */
export function eventsToText(events) {
  if (!Array.isArray(events)) return "";
  return events
    .filter((e) => e.type === "user.message" || e.role === "user")
    .map((e) => {
      if (typeof e.content === "string") return e.content;
      if (Array.isArray(e.content)) {
        return e.content
          .filter((c) => c.type === "text" || typeof c === "string")
          .map((c) => (typeof c === "string" ? c : c.text || ""))
          .join("");
      }
      return "";
    })
    .join("\n")
    .trim();
}
