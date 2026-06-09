import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import express from "express";
import { serializeFor } from "./runtime.mjs";
import { textDeltaFrame, idleFrame, errorFrame, eventsToText } from "./anthropic.mjs";

const PORT = Number(process.env.PORT) || 8080;
const DEFAULT_MODEL = process.env.LITELLM_DEFAULT_MODEL || "claude-sonnet-4-6";

// In-memory stores
const agents = new Map();   // id → { id, name, model, system }
const envs = new Map();     // id → { id }
const sessions = new Map(); // id → { id, agentId, envId, history, subscribers, activeProcess }

const log = (...a) => console.log("[hermes]", ...a);

// ── Per-session SSE emit ──────────────────────────────────────────────────────
// Delivers only to subscribers of the specific session — no cross-session leakage.

function emit(sessionId, frame) {
  const s = sessions.get(sessionId);
  if (!s) return;
  for (const cb of s.subscribers) { try { cb(frame); } catch {} }
}

// ── Hermes run turn ──────────────────────────────────────────────────────────

async function hermesRunTurn(sessionId, userText, agent) {
  const s = sessions.get(sessionId);
  if (!s) throw new Error(`session ${sessionId} not found`);

  const model = agent?.model || DEFAULT_MODEL;
  const system = agent?.system || "";

  // Build context from prior history
  const contextLines = [];
  if (system) contextLines.push(`System: ${system}`);
  for (const msg of s.history) {
    const role = msg.role === "assistant" ? "Assistant" : "User";
    if (msg.text) contextLines.push(`${role}: ${msg.text}`);
  }
  const fullPrompt = contextLines.length > 0
    ? `${contextLines.join("\n\n")}\n\nUser: ${userText}`
    : userText;

  const base = (process.env.LITELLM_API_BASE || "").replace(/\/+$/, "");
  const apiKey = process.env.LITELLM_API_KEY || "";

  let totalText = "";
  let lastError;

  await serializeFor(sessionId)(async () => {
    await new Promise((resolve, reject) => {
      const child = spawn(
        "hermes",
        ["chat", "--provider", "openai-api", "--model", model, "-q", fullPrompt],
        {
          env: { ...process.env, OPENAI_BASE_URL: base, OPENAI_API_KEY: apiKey },
          stdio: ["ignore", "pipe", "pipe"],
        },
      );
      s.activeProcess = child;

      child.stdout.on("data", (chunk) => {
        const delta = chunk.toString("utf8");
        if (!delta) return;
        totalText += delta;
        const frame = textDeltaFrame(delta, model);
        if (frame) emit(sessionId, frame);
      });

      child.stderr.on("data", (chunk) => {
        log(`stderr sid=${sessionId}: ${chunk.toString("utf8").slice(0, 200)}`);
      });

      child.on("exit", (code) => {
        s.activeProcess = null;
        if (code !== 0 && code !== null) reject(new Error(`hermes exited with code ${code}`));
        else resolve();
      });
      child.on("error", (err) => { s.activeProcess = null; reject(err); });
    });
  }).catch((err) => {
    const msg = err instanceof Error ? err.message : String(err);
    lastError = msg;
    log(`turn error sid=${sessionId}: ${msg}`);
  });

  // Record in history
  s.history.push({ role: "user", text: userText });
  s.history.push({ role: "assistant", text: totalText });

  if (lastError) {
    emit(sessionId, errorFrame(lastError));
  } else {
    emit(sessionId, idleFrame());
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
  const agent = { id, name: name || "agent", model: model?.id || model || DEFAULT_MODEL, system: system || "" };
  agents.set(id, agent);
  log(`agent created id=${id}`);
  res.json({ id, type: "agent", ...agent });
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
  if (model !== undefined) agent.model = model?.id || model;
  if (system !== undefined) agent.system = system;
  res.json(agent);
});

// POST /v1/environments
app.post("/v1/environments", (_req, res) => {
  const id = `env_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
  envs.set(id, { id });
  res.status(201).json({ id, type: "environment" });
});

// POST /v1/sessions
app.post("/v1/sessions", (req, res) => {
  const { agent_id, environment_id } = req.body || {};
  if (agent_id && !agents.has(agent_id)) {
    return res.status(404).json({ error: `agent not found: ${agent_id}` });
  }
  const id = `ses_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
  sessions.set(id, {
    id,
    agentId: agent_id || null,
    envId: environment_id || null,
    history: [],
    subscribers: new Set(),
    activeProcess: null,
  });
  log(`session created id=${id} agent=${agent_id}`);
  res.status(201).json({ id, type: "session", agent: agent_id, environment_id, status: "idle" });
});

// POST /v1/sessions/:id/events — send prompt (202)
app.post("/v1/sessions/:id/events", async (req, res) => {
  const s = sessions.get(req.params.id);
  if (!s) return res.status(404).json({ error: "session not found" });

  const body = req.body || {};

  // Accept: {events:[{type:"user.message",content:...}]} or {content:...} or {text:...}
  let text = "";
  if (Array.isArray(body.events)) {
    text = eventsToText(body.events);
  } else {
    text = typeof body.content === "string" ? body.content : (body.text ?? "");
  }
  if (!text.trim()) return res.status(400).json({ error: "no text in event" });

  const agent = s.agentId ? agents.get(s.agentId) : null;
  res.status(202).end();
  hermesRunTurn(req.params.id, text, agent).catch(e =>
    log(`runTurn error sid=${req.params.id}:`, e.message),
  );
});

// POST /v1/sessions/:id/abort
app.post("/v1/sessions/:id/abort", (req, res) => {
  const s = sessions.get(req.params.id);
  if (s?.activeProcess) {
    s.activeProcess.kill("SIGTERM");
    s.activeProcess = null;
    log(`abort sid=${req.params.id}`);
  }
  res.status(204).end();
});

// GET /v1/sessions/:id/events — stub
app.get("/v1/sessions/:id/events", (req, res) => {
  if (!sessions.has(req.params.id)) return res.status(404).json({ error: "not found" });
  res.json({ data: [] });
});

// GET /v1/sessions/:id/events/stream — per-session SSE
app.get("/v1/sessions/:id/events/stream", (req, res) => {
  const s = sessions.get(req.params.id);
  if (!s) return res.status(404).json({ error: "session not found" });

  res.writeHead(200, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "keep-alive",
  });
  res.flushHeaders?.();

  const push = (frame) => { try { res.write(frame); } catch {} };
  s.subscribers.add(push);

  req.on("close", () => s.subscribers.delete(push));
});

// ── Boot ──────────────────────────────────────────────────────────────────────

const server = createServer(app);
let shuttingDown = false;

function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  log("shutting down");
  for (const s of sessions.values()) {
    try { s.activeProcess?.kill("SIGTERM"); } catch {}
  }
  server.close(() => process.exit(0));
}

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

server.listen(PORT, "0.0.0.0", () => {
  log(`listening on 0.0.0.0:${PORT}`);
});
