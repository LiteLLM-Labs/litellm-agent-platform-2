import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import express from "express";
import { serialize, activeProcesses } from "./runtime.mjs";
import { translateRuntimeEvent, idleEvent, errorEvent } from "./anthropic.mjs";

const PORT = Number(process.env.PORT) || 8080;
const DEFAULT_MODEL = process.env.LITELLM_DEFAULT_MODEL || "claude-sonnet-4-6";

// In-memory stores
const agents = new Map();   // id → { id, name, model, system }
const envs = new Map();     // id → { id }
const sessions = new Map(); // id → { id, agentId, envId, history, subscribers, activeProcess }
const globalBus = new Set(); // SSE response writers for all sessions

const log = (...a) => console.log("[hermes]", ...a);

// ── SSE helpers ──────────────────────────────────────────────────────────────

function sseEvent(type, properties) {
  const ev = { id: `evt_${randomUUID().replace(/-/g, "").slice(0, 20)}`, type, properties };
  return `data: ${JSON.stringify(ev)}\n\n`;
}

function emit(sessionId, type, properties) {
  const line = sseEvent(type, { ...properties, sessionID: sessionId });
  const s = sessions.get(sessionId);
  if (s) for (const cb of s.subscribers) { try { cb(line); } catch {} }
  for (const cb of globalBus) { try { cb(line); } catch {} }
}

// ── Hermes run turn ──────────────────────────────────────────────────────────

async function hermesRunTurn(sessionId, userText, modelId) {
  const s = sessions.get(sessionId);
  if (!s) throw new Error(`session ${sessionId} not found`);

  const startedAt = Date.now();
  const userMsgId = `msg_${randomUUID().replace(/-/g, "").slice(0, 20)}`;
  const asstMsgId = `msg_${randomUUID().replace(/-/g, "").slice(0, 20)}`;
  const partId = `${asstMsgId}_b0`;

  // Record user message
  const userMsg = { info: { id: userMsgId, role: "user", time: { created: startedAt, completed: startedAt } }, parts: [{ id: `${userMsgId}_p0`, messageID: userMsgId, type: "text", text: userText }] };
  s.history.push(userMsg);
  emit(sessionId, "message.updated", { info: userMsg.info });
  emit(sessionId, "message.part.updated", { messageID: userMsgId, part: userMsg.parts[0] });
  emit(sessionId, "message.updated", { info: { id: asstMsgId, role: "assistant", time: { created: startedAt } } });
  emit(sessionId, "message.part.updated", { messageID: asstMsgId, part: { id: partId, messageID: asstMsgId, type: "text", text: "" } });

  // Build context from prior history
  const contextLines = [];
  for (const msg of s.history.slice(0, -1)) {
    const role = msg.info.role === "assistant" ? "Assistant" : "User";
    const text = (msg.parts || []).filter(p => p.type === "text").map(p => p.text).join("\n");
    if (text) contextLines.push(`${role}: ${text}`);
  }
  const fullPrompt = contextLines.length > 0
    ? `${contextLines.join("\n\n")}\n\nUser: ${userText}`
    : userText;

  const model = modelId || DEFAULT_MODEL;
  const base = (process.env.LITELLM_API_BASE || "").replace(/\/+$/, "");
  const apiKey = process.env.LITELLM_API_KEY || "";

  let totalText = "";
  let lastError;

  await serialize(async () => {
    await new Promise((resolve, reject) => {
      const child = spawn("hermes", ["chat", "--provider", "openai-api", "--model", model, "-q", fullPrompt], {
        env: { ...process.env, OPENAI_BASE_URL: base, OPENAI_API_KEY: apiKey },
        stdio: ["ignore", "pipe", "pipe"],
      });
      activeProcesses.set(sessionId, child);

      child.stdout.on("data", (chunk) => {
        const ev = translateRuntimeEvent(chunk, { model, messageId: asstMsgId, partId, sessionId });
        if (!ev) return;
        const delta = typeof chunk === "string" ? chunk : chunk.toString("utf8");
        totalText += delta;
        emit(sessionId, "message.part.delta", { messageID: asstMsgId, partID: partId, field: "text", delta });
      });

      child.stderr.on("data", (chunk) => {
        log(`stderr sid=${sessionId}: ${chunk.toString("utf8").slice(0, 200)}`);
      });

      child.on("exit", (code) => {
        activeProcesses.delete(sessionId);
        if (code !== 0 && code !== null) reject(new Error(`hermes exited with code ${code}`));
        else resolve();
      });
      child.on("error", (err) => { activeProcesses.delete(sessionId); reject(err); });
    });
  }).catch((err) => {
    const msg = err instanceof Error ? err.message : String(err);
    lastError = msg;
    log(`turn error sid=${sessionId}: ${msg}`);
  });

  const completedAt = Date.now();
  const textPart = { id: partId, messageID: asstMsgId, type: "text", text: totalText };
  const fullInfo = { id: asstMsgId, role: "assistant", time: { created: startedAt, completed: completedAt }, harness: "hermes", modelID: model, ...(lastError ? { error: { name: "HermesError", data: { message: lastError.slice(0, 500) } } } : { finish: "stop" }) };
  s.history.push({ info: fullInfo, parts: [textPart] });
  emit(sessionId, "message.updated", { info: fullInfo });
  if (lastError) {
    const ev = errorEvent(sessionId, lastError);
    emit(sessionId, ev.type, ev.properties);
  } else {
    const ev = idleEvent(sessionId);
    emit(sessionId, ev.type, ev.properties);
  }
  log(`turn done sid=${sessionId} chars=${totalText.length}`);
}

// ── Express app ───────────────────────────────────────────────────────────────

const app = express();
app.use(express.json());

// GET /health
app.get("/health", (_req, res) => {
  res.json({ ok: true, hermes: true });
});

// POST /v1/agents
app.post("/v1/agents", (req, res) => {
  const { name, model, system } = req.body || {};
  const id = `agent_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
  const agent = { id, name: name || "agent", model: model || DEFAULT_MODEL, system: system || "" };
  agents.set(id, agent);
  log(`agent created id=${id}`);
  res.json(agent);
});

// GET /v1/agents
app.get("/v1/agents", (_req, res) => {
  res.json({ data: [...agents.values()] });
});

// GET /v1/agents/:id
app.get("/v1/agents/:id", (req, res) => {
  const agent = agents.get(req.params.id);
  if (!agent) return res.status(404).json({ error: "not found" });
  res.json(agent);
});

// PATCH /v1/agents/:id
app.patch("/v1/agents/:id", (req, res) => {
  const agent = agents.get(req.params.id);
  if (!agent) return res.status(404).json({ error: "not found" });
  const { name, model, system } = req.body || {};
  if (name !== undefined) agent.name = name;
  if (model !== undefined) agent.model = model;
  if (system !== undefined) agent.system = system;
  res.json(agent);
});

// POST /v1/environments
app.post("/v1/environments", (_req, res) => {
  const id = `env_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
  envs.set(id, { id });
  res.status(201).json({ id });
});

// POST /v1/sessions
app.post("/v1/sessions", (req, res) => {
  const { agent_id, environment_id } = req.body || {};
  if (agent_id && !agents.has(agent_id)) {
    return res.status(404).json({ error: `agent not found: ${agent_id}` });
  }
  const id = `ses_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
  const session = { id, agentId: agent_id || null, envId: environment_id || null, history: [], subscribers: new Set(), activeProcess: null };
  sessions.set(id, session);
  log(`session created id=${id} agent=${agent_id}`);
  res.status(201).json({ id, agent_id, environment_id });
});

// POST /v1/sessions/:id/events — send prompt (202)
app.post("/v1/sessions/:id/events", async (req, res) => {
  const s = sessions.get(req.params.id);
  if (!s) return res.status(404).json({ error: "session not found" });

  const body = req.body || {};
  // Accept multiple shapes: {events:[{type:"user.message",content:...}]}, {type:"human",content:...}, {content:...}
  let text = "";
  if (Array.isArray(body.events)) {
    text = body.events
      .filter(e => e.type === "user.message" || e.role === "user")
      .map(e => typeof e.content === "string" ? e.content : (Array.isArray(e.content) ? e.content.filter(c => c.type === "text").map(c => c.text).join("") : ""))
      .join("\n");
  } else {
    text = typeof body.content === "string" ? body.content : (body.text ?? "");
  }
  if (!text.trim()) return res.status(400).json({ error: "no text in event" });

  const agent = s.agentId ? agents.get(s.agentId) : null;
  const modelId = agent?.model || DEFAULT_MODEL;

  res.status(202).end();
  hermesRunTurn(req.params.id, text, modelId).catch(e => log(`runTurn error sid=${req.params.id}:`, e.message));
});

// POST /v1/sessions/:id/abort
app.post("/v1/sessions/:id/abort", (req, res) => {
  const child = activeProcesses.get(req.params.id);
  if (child) { child.kill("SIGTERM"); activeProcesses.delete(req.params.id); log(`abort sid=${req.params.id}`); }
  res.status(204).end();
});

// GET /v1/sessions/:id/events — stub
app.get("/v1/sessions/:id/events", (req, res) => {
  if (!sessions.has(req.params.id)) return res.status(404).json({ error: "not found" });
  res.json({ data: [] });
});

// GET /v1/sessions/:id/events/stream — live SSE
app.get("/v1/sessions/:id/events/stream", (req, res) => {
  const s = sessions.get(req.params.id);
  if (!s) return res.status(404).json({ error: "session not found" });

  res.writeHead(200, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "keep-alive",
  });

  const push = (line) => { try { res.write(line); } catch {} };
  s.subscribers.add(push);
  globalBus.add(push);

  req.on("close", () => {
    s.subscribers.delete(push);
    globalBus.delete(push);
  });
});

// ── Boot ──────────────────────────────────────────────────────────────────────

const server = createServer(app);
let shuttingDown = false;

function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  log("shutting down");
  for (const child of activeProcesses.values()) { try { child.kill("SIGTERM"); } catch {} }
  server.close(() => process.exit(0));
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

server.listen(PORT, "0.0.0.0", () => {
  log(`listening on 0.0.0.0:${PORT}`);
});
