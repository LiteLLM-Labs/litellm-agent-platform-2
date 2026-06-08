// Express entry: exposes opencode via the Anthropic Managed Agents API spec.
// Boots a child `opencode serve`, persists agents durably, provisions opencode
// per session, and translates opencode SSE -> Anthropic event shapes.
import express from "express";
import crypto from "node:crypto";
import { mkdirSync } from "node:fs";

import { createStore } from "./store.mjs";
import { startOpencode, provisionAgent, ocFetch } from "./opencode.mjs";
import {
  modelId,
  agentResponse,
  sessionResponse,
  partsFromEvents,
  translateOpencodeEvent,
} from "./anthropic.mjs";

// ---- boot config ----------------------------------------------------------
const PORT = process.env.PORT || 8080;
const OC_PORT = Number(process.env.OPENCODE_PORT || 4096);
const WORKDIR = process.env.WORKDIR || "/tmp/opencode-workspace";
const DB_PATH = process.env.DB_PATH || "/data/agents.db";

mkdirSync(WORKDIR, { recursive: true });

const store = createStore(DB_PATH);

console.log(`[boot] starting opencode on port ${OC_PORT} (cwd=${WORKDIR})`);
const oc = await startOpencode({ port: OC_PORT, cwd: WORKDIR });
console.log(`[boot] opencode ready at ${oc.baseUrl}`);

// In-memory environments registry (envId -> config).
const environments = new Map();

// ---- app ------------------------------------------------------------------
const app = express();
app.use(express.json({ limit: "5mb" }));

// Honor (but don't strictly require) Anthropic-style headers.
app.use((req, _res, next) => {
  req.apiKey = req.get("x-api-key") || null;
  req.anthropicVersion = req.get("anthropic-version") || null;
  req.anthropicBeta = req.get("anthropic-beta") || null;
  next();
});

// Wrap async handlers so throws become 500 {error}.
const wrap = (fn) => (req, res) =>
  Promise.resolve(fn(req, res)).catch((err) => {
    console.error(`[error] ${req.method} ${req.path}:`, err);
    if (!res.headersSent) res.status(500).json({ error: String(err?.message || err) });
    else try { res.end(); } catch {}
  });

// ---- health ---------------------------------------------------------------
app.get("/health", wrap(async (_req, res) => {
  let opencode = false;
  try {
    const r = await ocFetch(oc.baseUrl, "/global/health", {});
    opencode = !!r?.ok;
  } catch {
    opencode = false;
  }
  res.json({ ok: true, opencode });
}));

// ---- agents ---------------------------------------------------------------
app.post("/v1/agents", wrap(async (req, res) => {
  const { name, model, system } = req.body || {};
  const row = store.createAgent({
    name,
    system: system || "",
    model: modelId(model),
    permissions: req.body.permissions || {},
    mcp_servers: req.body.mcp_servers || [],
    workspace: null,
  });
  res.json(agentResponse(row));
}));

app.get("/v1/agents", wrap(async (_req, res) => {
  res.json({ data: store.listAgents().map(agentResponse) });
}));

app.get("/v1/agents/:id", wrap(async (req, res) => {
  const row = store.getAgent(req.params.id);
  if (!row) return res.status(404).json({ error: "agent not found" });
  res.json(agentResponse(row));
}));

// ---- environments ---------------------------------------------------------
app.post("/v1/environments", wrap(async (req, res) => {
  const { name, config } = req.body || {};
  const id = "env_" + crypto.randomBytes(16).toString("hex");
  environments.set(id, config || {});
  res.json({ id, type: "environment", name, config: config || {} });
}));

// ---- sessions -------------------------------------------------------------
app.post("/v1/sessions", wrap(async (req, res) => {
  const row = store.getAgent(req.body?.agent);
  if (!row) return res.status(400).json({ error: "unknown agent" });

  await provisionAgent(WORKDIR, row);

  const r = await ocFetch(oc.baseUrl, "/session", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: req.body.title || row.name + " session" }),
  });
  const ses = await r.json();
  const sid = ses.id;

  store.bindSession(sid, row.id);

  res.json(
    sessionResponse({
      id: sid,
      agentId: row.id,
      environmentId: req.body.environment_id,
    })
  );
}));

// Submit events (user.message parts) -> opencode prompt_async.
app.post("/v1/sessions/:id/events", wrap(async (req, res) => {
  const agentId = store.getSessionAgent(req.params.id);
  const agent = agentId ? store.getAgent(agentId) : null;

  const parts = partsFromEvents(req.body?.events || []);
  if (!parts.length) return res.status(400).json({ error: "no user.message parts" });

  await ocFetch(oc.baseUrl, `/session/${req.params.id}/prompt_async`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model: agent?.model || undefined,
      system: agent?.system || undefined,
      parts,
    }),
  });

  res.status(202).json({ ok: true });
}));

// Historical events (stub).
app.get("/v1/sessions/:id/events", wrap(async (_req, res) => {
  res.json({ data: [] });
}));

// Live SSE stream: opencode events -> Anthropic event shapes.
app.get("/v1/sessions/:id/events/stream", wrap(async (req, res) => {
  const agentId = store.getSessionAgent(req.params.id);
  const agent = agentId ? store.getAgent(agentId) : null;
  const model = agent?.model || null;

  res.setHeader("content-type", "text/event-stream");
  res.setHeader("cache-control", "no-cache");
  res.setHeader("connection", "keep-alive");
  res.flushHeaders?.();

  const controller = new AbortController();
  req.on("close", () => controller.abort());

  try {
    const upstream = await ocFetch(oc.baseUrl, "/event", { signal: controller.signal });

    const decoder = new TextDecoder();
    let buffer = "";

    for await (const chunk of upstream.body) {
      buffer += decoder.decode(chunk, { stream: true });

      // Consume only complete \n\n-delimited records.
      let idx;
      while ((idx = buffer.indexOf("\n\n")) !== -1) {
        const block = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);

        // Collect the data: line(s) within this block.
        const data = block
          .split("\n")
          .filter((l) => l.startsWith("data:"))
          .map((l) => l.slice(5).trim())
          .join("");
        if (!data) continue;

        let ev;
        try {
          ev = JSON.parse(data);
        } catch {
          continue;
        }

        const out = translateOpencodeEvent(ev, { sessionId: req.params.id, model });
        if (out && out.event) {
          res.write(`event: ${out.event}\ndata: ${JSON.stringify(out.data)}\n\n`);
        }
      }
    }
  } catch (err) {
    // Swallow abort errors; don't surface to a half-open stream.
    if (err?.name !== "AbortError" && !controller.signal.aborted) {
      console.error(`[stream] ${req.params.id}:`, err);
    }
  } finally {
    try { res.end(); } catch {}
  }
}));

// ---- listen + lifecycle ---------------------------------------------------
const server = app.listen(PORT, () => {
  console.log(`[boot] agent server listening on :${PORT}`);
});

let shuttingDown = false;
const shutdown = async (sig) => {
  if (shuttingDown) return;
  shuttingDown = true;
  console.log(`[shutdown] received ${sig}, stopping...`);
  try { server.close(); } catch {}
  try { await oc.stop(); } catch (e) { console.error("[shutdown] oc.stop:", e); }
  process.exit(0);
};
process.on("SIGTERM", () => shutdown("SIGTERM"));
process.on("SIGINT", () => shutdown("SIGINT"));
