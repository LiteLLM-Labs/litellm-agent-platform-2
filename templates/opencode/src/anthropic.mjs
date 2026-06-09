// Pure-logic translation layer between the Anthropic Managed Agents API spec
// and opencode. No external deps, no I/O — just data mapping. ESM (Node 20).

/**
 * Resolve a model identifier from a string or {id, speed?} shape.
 * @param {string|{id?: string}} model
 * @returns {string}
 */
export function modelId(model) {
  if (typeof model === "string") return model;
  if (model && typeof model === "object") return model.id || "";
  return "";
}

/**
 * Map a store agent row to Anthropic-shaped agent JSON.
 */
export function agentResponse(row) {
  return {
    id: row.id,
    type: "agent",
    name: row.name,
    description: row.description ?? null,
    model: { id: row.model || "" },
    system: row.system || "",
    tools: row.tools || [],
    mcp_servers: row.mcp_servers || [],
    metadata: row.metadata ?? null,
    version: 1,
    created_at: row.created_at,
    updated_at: row.updated_at,
  };
}

/**
 * Build an Anthropic session JSON object.
 */
export function sessionResponse({ id, agentId, environmentId }) {
  return {
    id,
    type: "session",
    agent: agentId,
    environment_id: environmentId ?? null,
    status: "running",
  };
}

/**
 * Collect text from Anthropic user.message events into opencode text parts.
 * @param {Array<{type: string, content?: any}>} events
 * @returns {Array<{type: "text", text: string}>}
 */
export function partsFromEvents(events) {
  const parts = [];
  if (!Array.isArray(events)) return parts;
  for (const ev of events) {
    if (!ev || ev.type !== "user.message") continue;
    const content = ev.content;
    if (typeof content === "string") {
      if (content) parts.push({ type: "text", text: content });
    } else if (Array.isArray(content)) {
      for (const item of content) {
        if (typeof item === "string") {
          if (item) parts.push({ type: "text", text: item });
        } else if (item && item.type === "text" && item.text) {
          parts.push({ type: "text", text: item.text });
        }
      }
    }
  }
  return parts;
}

/**
 * Translate a single opencode SSE event into an Anthropic {event, data} pair.
 * Returns null when the event should be dropped (other session / no mapping).
 * @param {object} raw opencode event ({type, properties} or flat)
 * @param {{sessionId?: string, model?: string}} ctx
 * @returns {{event: string, data: object}|null}
 */
export function translateOpencodeEvent(raw, ctx) {
  if (!raw || typeof raw !== "object") return null;
  const props = raw.properties || raw;

  // Resolve the event's session id from the various known locations.
  const sid =
    raw.properties?.sessionID ??
    raw.sessionID ??
    raw.properties?.session_id ??
    raw.properties?.info?.sessionID ??
    raw.properties?.part?.sessionID ??
    raw.properties?.message?.sessionID;

  // Filter out events that clearly belong to another session.
  if (sid != null && sid !== ctx.sessionId) return null;

  switch (raw.type) {
    // Stream assistant tokens from deltas only. `message.part.updated` is
    // skipped: it fires for the echoed user message and again as the final
    // assistant duplicate, so emitting it would double-send and echo input.
    case "message.part.delta": {
      const text =
        props.delta?.text ||
        (typeof props.delta === "string" ? props.delta : "") ||
        "";
      if (!text) return null;
      return {
        event: "agent.message",
        data: {
          content: [{ type: "text", text }],
          model: ctx.model || null,
        },
      };
    }
    case "message.part.updated": {
      // Tool calls arrive as updated parts — surface them as agent.tool_use.
      // Text updates are skipped (deltas already streamed them).
      const part = props.part || {};
      if (part.type === "tool" || part.tool) {
        return {
          event: "agent.tool_use",
          data: {
            tool: part.tool ?? null,
            input: part.state?.input ?? null,
            status: part.state?.status ?? null,
          },
        };
      }
      return null;
    }
    case "session.status": {
      const status = props.status?.type;
      if (status === "busy") {
        return { event: "session.status_running", data: {} };
      }
      if (status === "idle") {
        return {
          event: "session.status_idle",
          data: { stop_reason: { type: "end_turn" } },
        };
      }
      return null;
    }
    case "session.idle":
      return {
        event: "session.status_idle",
        data: { stop_reason: { type: "end_turn" } },
      };
    case "session.error":
      return {
        event: "session.error",
        data: {
          error: { message: props.error?.message || props.message || "error" },
        },
      };
    default: {
      // Best-effort tool-use mapping.
      const isTool =
        props.part?.type === "tool" ||
        (typeof raw.type === "string" && raw.type.includes("tool"));
      if (isTool) {
        return {
          event: "agent.tool_use",
          data: {
            tool: props.part?.tool ?? null,
            input: props.part?.state?.input ?? null,
          },
        };
      }
      return null;
    }
  }
}
