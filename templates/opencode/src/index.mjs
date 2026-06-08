// index.mjs — opencode-compatible wrapper HTTP server with a durable agents store.
//
// Boots a child `opencode serve` (via ./opencode.mjs), exposes a public product
// API for managing agents + sessions, and proxies session/message/event traffic
// to opencode while injecting per-agent config (agent name, model, system prompt).
//
// Node 20, ESM, Express. No deps beyond express; the store/opencode siblings own
// their own dependencies. ~220 lines.

import { mkdir } from "node:fs/promises";
import express from "express";
import { createStore } from "./store.mjs";
import { startOpencode, provisionAgent, ocFetch } from "./opencode.mjs";

const PORT = process.env.PORT || 8080;
const OC_PORT = Number(process.env.OPENCODE_PORT || 4096);
const WORKDIR = process.env.WORKDIR || "/tmp/opencode-workspace";
const DB_PATH = process.env.DB_PATH || "/data/agents.db";

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

await mkdir(WORKDIR, { recursive: true });
console.log(`[boot] workdir ready at ${WORKDIR}`);

const store = createStore(DB_PATH);
console.log(`[boot] agents store opened at ${DB_PATH}`);

const oc = await startOpencode({ port: OC_PORT, cwd: WORKDIR });
console.log(`[boot] opencode serving at ${oc.baseUrl}`);

const app = express();
app.use(express.json({ limit: "5mb" }));

// Wrap an async handler so thrown errors become a 500 JSON response.
const h = (fn) => (req, res) =>
  Promise.resolve(fn(req, res)).catch((err) => {
    console.error(`[error] ${req.method} ${req.path}:`, err);
    if (!res.headersSent) res.status(500).json({ error: String(err?.message || err) });
  });

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

app.get(
  "/health",
  h(async (_req, res) => {
    let opencodeOk = false;
    try {
      const r = await ocFetch(oc.baseUrl, "/health", {});
      opencodeOk = r.ok;
    } catch {
      opencodeOk = false;
    }
    res.json({ ok: true, opencode: opencodeOk });
  })
);

// ---------------------------------------------------------------------------
// Agents CRUD
// ---------------------------------------------------------------------------

app.post(
  "/agents",
  h(async (req, res) => {
    const { name, system, model, permissions, mcp_servers, workspace } = req.body || {};
    const row = store.createAgent({ name, system, model, permissions, mcp_servers, workspace });
    res.json(row);
  })
);

app.get(
  "/agents",
  h(async (_req, res) => {
    res.json(store.listAgents());
  })
);

app.get(
  "/agents/:id",
  h(async (req, res) => {
    const row = store.getAgent(req.params.id);
    if (!row) return res.status(404).json({ error: "not found" });
    res.json(row);
  })
);

app.patch(
  "/agents/:id",
  h(async (req, res) => {
    const row = store.updateAgent(req.params.id, req.body || {});
    if (!row) return res.status(404).json({ error: "not found" });
    res.json(row);
  })
);

app.delete(
  "/agents/:id",
  h(async (req, res) => {
    const ok = store.deleteAgent(req.params.id);
    if (!ok) return res.status(404).json({ error: "not found" });
    res.json({ deleted: true });
  })
);

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

app.post(
  "/session",
  h(async (req, res) => {
    const body = req.body || {};
    const agent = store.getAgent(body.agent);
    if (!agent) return res.status(400).json({ error: "unknown agent" });

    await provisionAgent(WORKDIR, agent);

    const title = body.title || `${agent.name} session`;
    const ocRes = await ocFetch(oc.baseUrl, "/session", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title }),
    });
    const ocSession = await ocRes.json();
    const ocSessionId = ocSession.id;

    store.bindSession(ocSessionId, agent.id);

    res.json({ id: ocSessionId, agent: agent.id, harness: body.harness || "opencode" });
  })
);

// Shared builder for /message and /prompt_async: resolves the bound agent and
// assembles the opencode message body with per-agent injection.
function buildMessageBody(req) {
  const agentId = store.getSessionAgent(req.params.id);
  const agent = agentId ? store.getAgent(agentId) : null;
  return {
    agent: agentId || undefined,
    model: req.body?.model || agent?.model || undefined,
    system: agent?.system || undefined,
    parts: req.body?.parts,
  };
}

app.post(
  "/session/:id/message",
  h(async (req, res) => {
    const body = buildMessageBody(req);
    const ocRes = await ocFetch(oc.baseUrl, `/session/${req.params.id}/message`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const json = await ocRes.json();
    res.status(ocRes.status).json(json);
  })
);

app.post(
  "/session/:id/prompt_async",
  h(async (req, res) => {
    const body = buildMessageBody(req);
    await ocFetch(oc.baseUrl, `/session/${req.params.id}/prompt_async`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    res.status(204).end();
  })
);

app.post(
  "/session/:id/abort",
  h(async (req, res) => {
    const ocRes = await ocFetch(oc.baseUrl, `/session/${req.params.id}/abort`, {
      method: "POST",
    });
    res.status(ocRes.status);
    const text = await ocRes.text();
    if (text) res.send(text);
    else res.end();
  })
);

app.delete(
  "/session/:id",
  h(async (req, res) => {
    const ocRes = await ocFetch(oc.baseUrl, `/session/${req.params.id}`, {
      method: "DELETE",
    });
    store.unbindSession(req.params.id);
    res.status(ocRes.status);
    const text = await ocRes.text();
    if (text) res.send(text);
    else res.end();
  })
);

// ---------------------------------------------------------------------------
// Event stream (SSE proxy)
// ---------------------------------------------------------------------------

app.get(
  "/event",
  h(async (req, res) => {
    const controller = new AbortController();
    req.on("close", () => controller.abort());

    const upstream = await ocFetch(oc.baseUrl, "/event", { signal: controller.signal });
    res.set({
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
    });
    res.flushHeaders?.();

    try {
      for await (const chunk of upstream.body) res.write(chunk);
    } catch (err) {
      if (!controller.signal.aborted) throw err;
    } finally {
      res.end();
    }
  })
);

// ---------------------------------------------------------------------------
// Listen + graceful shutdown
// ---------------------------------------------------------------------------

const server = app.listen(PORT, () => {
  console.log(`[boot] wrapper listening on :${PORT}`);
});

async function shutdown(signal) {
  console.log(`[shutdown] received ${signal}, stopping...`);
  try {
    server.close();
  } catch {}
  try {
    await oc.stop();
  } catch (err) {
    console.error("[shutdown] opencode stop error:", err);
  }
  process.exit(0);
}

process.on("SIGTERM", () => shutdown("SIGTERM"));
process.on("SIGINT", () => shutdown("SIGINT"));
