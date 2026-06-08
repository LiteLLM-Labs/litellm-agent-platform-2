# opencode-agent-server

A durable, opencode-compatible agent server. Register agents once (system prompt, model, tool permissions, MCP servers); run sessions against them over HTTP.

## Architecture

- **Wrapper server** — a small Express app that exposes a product API for agents + sessions and proxies session/message/event traffic to opencode, injecting per-agent system prompt, model, and tool permissions.
- **Durable agent store** — agent definitions and session→agent bindings persisted in SQLite (`better-sqlite3`, WAL) at `DB_PATH`, so agents survive restarts.
- **Child opencode** — the server boots one `opencode serve` child and provisions per-agent config (an agent `.md` file plus `opencode.json` MCP entries) into the workspace before each session.

## Quickstart

### Docker

```bash
docker build -t opencode-agent-server .
docker run -p 8080:8080 -e ANTHROPIC_API_KEY=sk-... opencode-agent-server
```

### Local (Node 20+)

The opencode CLI must be installed and on `PATH`:

```bash
npm i -g opencode-ai
```

Then:

```bash
npm install
ANTHROPIC_API_KEY=... npm start
```

> **Model provider key required.** To actually answer prompts, the child opencode needs a model provider key in the server's environment (e.g. `ANTHROPIC_API_KEY` for `anthropic/*` models, `OPENAI_API_KEY` for `openai/*`, etc.). Without it, agents register and sessions create fine, but prompts will not produce assistant output.

## API reference

Base URL defaults to `http://localhost:8080`.

| Method | Path | Body | Returns |
| --- | --- | --- | --- |
| `GET` | `/health` | — | `{ ok: true, opencode: bool }` |
| `POST` | `/agents` | `{name, system, model, permissions?, mcp_servers?, workspace?}` | agent row `{id:"agt_...", ...}` |
| `GET` | `/agents` | — | `[agent, ...]` |
| `GET` | `/agents/:id` | — | agent row |
| `PATCH` | `/agents/:id` | partial agent fields | updated agent row |
| `DELETE` | `/agents/:id` | — | `{deleted:true}` |
| `POST` | `/session` | `{title?, agent:"agt_...", harness?}` | `{id, agent, harness}` |
| `POST` | `/session/:id/message` | `{model?, parts:[{type:"text",text}]}` | opencode `{info, parts}` (sync) |
| `POST` | `/session/:id/prompt_async` | same as message | `204 No Content` |
| `POST` | `/session/:id/abort` | — | opencode abort result |
| `DELETE` | `/session/:id` | — | opencode delete result |
| `GET` | `/event` | — | SSE stream (opencode event shapes) |

### Health

```bash
curl -s http://localhost:8080/health
# {"ok":true,"opencode":true}
```

### Create an agent

```bash
curl -s http://localhost:8080/agents \
  -H 'content-type: application/json' \
  -d '{
    "name": "Terse Assistant",
    "system": "You are a terse assistant.",
    "model": "anthropic/claude-sonnet-4-5",
    "permissions": { "bash": "deny", "edit": "deny" }
  }'
# {"id":"agt_...","name":"Terse Assistant","system":"...","model":"...","permissions":{...},"mcp_servers":[],"workspace":null,...}
```

### List / get / update / delete agents

```bash
curl -s http://localhost:8080/agents
curl -s http://localhost:8080/agents/agt_abc123
curl -s -X PATCH http://localhost:8080/agents/agt_abc123 \
  -H 'content-type: application/json' \
  -d '{"model":"anthropic/claude-opus-4-1"}'
curl -s -X DELETE http://localhost:8080/agents/agt_abc123
```

### Create a session

```bash
curl -s http://localhost:8080/session \
  -H 'content-type: application/json' \
  -d '{"agent":"agt_abc123"}'
# {"id":"ses_...","agent":"agt_abc123","harness":"opencode"}
```

### Send a message (synchronous)

Blocks until opencode finishes, then returns the opencode `{info, parts}` shape:

```bash
curl -s http://localhost:8080/session/ses_xyz/message \
  -H 'content-type: application/json' \
  -d '{"parts":[{"type":"text","text":"Say hello in 3 words."}]}'
```

### Prompt asynchronously

Returns `204` immediately; observe output on `/event`:

```bash
curl -s -X POST http://localhost:8080/session/ses_xyz/prompt_async \
  -H 'content-type: application/json' \
  -d '{"parts":[{"type":"text","text":"Say hello in 3 words."}]}'
```

### Stream events (SSE)

A passthrough of opencode's event stream. Filter client-side on `properties.sessionID`:

```bash
curl -sN http://localhost:8080/event
```

### Abort / delete a session

```bash
curl -s -X POST http://localhost:8080/session/ses_xyz/abort
curl -s -X DELETE http://localhost:8080/session/ses_xyz
```

## End-to-end example

```bash
#!/usr/bin/env bash
set -euo pipefail
BASE=${BASE:-http://localhost:8080}

# 1. Create an agent.
AGENT=$(curl -s "$BASE/agents" \
  -H 'content-type: application/json' \
  -d '{"name":"Demo","system":"You are a terse assistant.","model":"anthropic/claude-sonnet-4-5","permissions":{"bash":"deny","edit":"deny"}}')
AID=$(printf '%s' "$AGENT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
echo "agent: $AID"

# 2. Create a session bound to that agent.
SESSION=$(curl -s "$BASE/session" \
  -H 'content-type: application/json' \
  -d "{\"agent\":\"$AID\"}")
SID=$(printf '%s' "$SESSION" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
echo "session: $SID"

# 3. Subscribe to /event in the background.
curl -sN "$BASE/event" > /tmp/events.log &
EV=$!
sleep 1

# 4. Prompt asynchronously.
curl -s -X POST "$BASE/session/$SID/prompt_async" \
  -H 'content-type: application/json' \
  -d '{"parts":[{"type":"text","text":"Say hello in 3 words."}]}'

# 5. Wait, then read the events that arrived.
sleep 8
kill "$EV" 2>/dev/null || true
echo "--- events ---"
cat /tmp/events.log
```

## Agent config fields

| Field | Type | Description |
| --- | --- | --- |
| `name` | string | Human label for the agent. |
| `system` | string | System prompt injected for every session/message. |
| `model` | string | `"provider/model"`, e.g. `"anthropic/claude-sonnet-4-5"`. |
| `permissions` | object | Per-tool gating, e.g. `{ "bash": "deny", "edit": "ask" }`. Values: `"deny" \| "allow" \| "ask"`. |
| `mcp_servers` | array | MCP servers. Remote: `{ "name": "...", "url": "https://..." }`. Local: `{ "name": "...", "command": "npx", "args": ["..."] }`. |
| `workspace` | object | Optional repo context: `{ "repository": "...", "ref": "..." }`. |

## Environment variables

| Var | Default | Purpose |
| --- | --- | --- |
| `PORT` | `8080` | Port the wrapper server listens on. |
| `OPENCODE_PORT` | `4096` | Port for the child `opencode serve`. |
| `WORKDIR` | `/tmp/opencode-workspace` | Working directory opencode runs in; where per-agent config is provisioned. |
| `DB_PATH` | `/data/agents.db` | SQLite file for the durable agent store. |
| `ANTHROPIC_API_KEY` (or other provider key) | — | Passed through to opencode so it can call the model. Required to answer prompts. |

## Deploy to Render

Deploy as a **Docker web service** (a `render.yaml` blueprint is included):

1. Create a new **Web Service** from this repo, runtime **Docker**, root dir `templates/opencode`.
2. Set the health check path to `/health`.
3. **Mount a disk for durability.** The SQLite store lives at `DB_PATH`; without a persistent disk the agent database is wiped on every deploy. Add a disk and point `DB_PATH` at its mount path (the blueprint mounts a 1 GB disk at `/var/data` and sets `DB_PATH=/var/data/agents.db`).
4. Set a **model provider key** (e.g. `ANTHROPIC_API_KEY`) as an environment variable so opencode can answer prompts.

## Where this fits

This server is the standalone **"Layer 2"** that the lite-harness / LAP SDK talks to via the opencode-compatible HTTP contract. It owns durable agent definitions and session orchestration; callers drive it entirely over the HTTP API documented above.
