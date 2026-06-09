# opencode behind the Anthropic Managed Agents API

This server exposes [opencode](https://opencode.ai) through the **Anthropic Managed Agents API spec**. Any client that already speaks that spec — including the **LiteLLM Agent Platform (LAP) SDK's `claude_managed_agents` runtime** — drives it with **zero code changes**: just point `api_base` + `api_key` at this server. Under the hood the server translates Anthropic Managed Agents calls into opencode: it spawns `opencode serve`, provisions per-agent config, proxies prompts, and translates opencode's SSE events back into Anthropic event shapes.

In other words: to the SDK this looks exactly like Anthropic's Managed Agents service. opencode is an implementation detail nobody on the client side ever sees.

## Why this design

The whole point is that the **front speaks the Anthropic Managed Agents spec**, so a client SDK needs no opencode-specific code — and the SDK's mature, already-shipped `claude_managed_agents` path is reused verbatim, no second runtime, no adapter, no fork. Every method the SDK already has maps cleanly onto opencode:

- `create_agent` → durable agent record + a provisioned `.opencode/agent/<id>.md` (system prompt, model, permissions).
- `create_environment` → a named workspace config (optionally a git `repository`/`ref`).
- `create_session` → boots/attaches the child opencode for that agent and opens an opencode session.
- `events().send(...)` → forwards a `user.message` to opencode as a prompt.
- `events().stream(...)` → opencode's SSE is translated into Anthropic event types (`agent.message`, `agent.thinking`, `agent.tool_use`, `agent.tool_result`, `session.status_*`, `session.error`).

Because the contract is identical, you change only `api_base`/`api_key` and the existing managed-agents code path drives opencode.

## Architecture

- **Anthropic-spec front** — an Express app that implements the Anthropic Managed Agents endpoints (`POST /v1/agents`, `POST /v1/environments`, `POST /v1/sessions`, `POST /v1/sessions/:id/events`, `GET /v1/sessions/:id/events/stream`, …) and honors `x-api-key` / `anthropic-version` / `anthropic-beta: managed-agents-2026-04-01`.
- **Durable agent store** — agents, environments, and session→agent bindings persisted in SQLite (`better-sqlite3`, WAL) at `DB_PATH`, so agents survive restarts and keep stable `agt_…` / `env_…` / session ids.
- **Child opencode provisioned per session + SSE translation** — the server boots one `opencode serve` child, provisions per-agent config (an agent `.md` file plus `opencode.json` MCP entries) per session, proxies prompts to it, and rewrites opencode's event stream into Anthropic event frames.

## Quickstart

### Docker

```bash
docker build -t opencode-anthropic-server .
docker run -p 8080:8080 -e ANTHROPIC_API_KEY=sk-ant-... opencode-anthropic-server
```

### Local (Node 20+)

The opencode CLI must be installed and on `PATH`:

```bash
npm i -g opencode-ai
```

Then:

```bash
npm install && ANTHROPIC_API_KEY=... npm start
```

> **Model provider key required.** To actually answer prompts, the child opencode needs a model provider key in the server's environment (e.g. `ANTHROPIC_API_KEY` for `anthropic/*` models). Without it, agents/environments/sessions create fine, but prompts won't produce assistant output. This key is the *server's* — it is not the `x-api-key` your clients send.

## The killer demo — the LAP SDK, no new code

The LiteLLM Agent Platform SDK already ships a `claude_managed_agents` runtime that speaks the Anthropic Managed Agents spec. Point it at this server and it drives opencode without a single new line of integration code:

```rust
let lap = Lap::new(LapConfig {
    anthropic_api_key: Some("any-key".into()),
    anthropic_base_url: "https://<this-server>".into(),
    ..Default::default()
});
// runtime: claude_managed_agents
let agent = lap.beta().agents().create(/* name, model, system */).await?;
let session = lap.beta().sessions().create(/* agent, environment */).await?;
lap.beta().sessions().events().send(&session.id, /* user.message */).await?;
let mut stream = lap.beta().sessions().events().stream(&session.id).await?;
```

> opencode, driven through the Anthropic Managed Agents SDK path, by changing only `api_base`/`api_key`.

## Calling with the LAP SDK

Full end-to-end example using the LiteLLM Agent Platform Rust SDK (`litellm_rust`).
The **only** server-specific config is `anthropic_base_url` + `anthropic_api_key`;
everything else is the SDK's normal `claude_managed_agents` flow.

```rust
use litellm_rust::sdk::agents::{
    AgentModel, AgentRuntime, CreateAgentParams, CreateEnvironmentParams,
    CreateSessionParams, Lap, LapConfig, SendEventsParams,
};
use futures_util::StreamExt;
use serde_json::json;

// 1. Point the SDK at this server (key is accepted loosely).
let lap = Lap::new(LapConfig {
    anthropic_api_key: Some("any-key".into()),
    anthropic_base_url: "https://<this-server>".into(), // e.g. http://localhost:8080
    ..LapConfig::default()
});

// 2. Create an agent. Use the gateway provider id in the model, e.g. "litellm/<model>".
let agent = lap.beta().agents().create(CreateAgentParams {
    lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
    lap_provider_options: None,
    name: "Demo".into(),
    model: AgentModel::from("litellm/claude-sonnet-4-5"),
    system: "You are a terse assistant.".into(),
    description: None,
    tools: Vec::new(),
    mcp_servers: Vec::new(),   // e.g. [{ "name": "deepwiki", "url": "https://mcp.deepwiki.com/mcp" }]
    env_vars: None,
    workspace: None,
    metadata: None,
}).await?;

// 3. Environment (a named workspace; config is optional).
let env = lap.beta().environments().create(CreateEnvironmentParams {
    lap_agent_runtime: AgentRuntime::ClaudeManagedAgents,
    name: "demo-env".into(),
    config: json!({}),
    description: None,
    scope: None,
}).await?;

// 4. Session bound to the agent (this provisions opencode for it).
let session = lap.beta().sessions().create(CreateSessionParams {
    agent: agent.id.clone(),
    environment_id: env.id.clone(),
    title: "demo session".into(),
    lap_agent_runtime: Some(AgentRuntime::ClaudeManagedAgents),
    metadata: None,
    vault_ids: None,
    resources: None,
}).await?;

// 5. Send a user message.
lap.beta().sessions().events().send(&session.id, SendEventsParams {
    events: vec![json!({
        "type": "user.message",
        "content": [{ "type": "text", "text": "Name the three primary colors." }]
    })],
}).await?;

// 6. Stream the reply (Anthropic event types: agent.message, session.status_*, agent.tool_use, ...).
let mut stream = lap.beta().sessions().events().stream(&session.id).await?;
while let Some(Ok(event)) = stream.next().await {
    if event.event_type == "agent.message" {
        if let Some(blocks) = event.data.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                if let Some(t) = b.get("text").and_then(|t| t.as_str()) { print!("{t}"); }
            }
        }
    }
    if event.event_type == "session.status_idle" { break; }
}
```

A runnable version lives at `tests/opencode_anthropic_server_live.rs` in the
parent repo (a `#[ignore]`d live test). With the server running:

```bash
OPENCODE_ANTHROPIC_BASE=http://localhost:8080 \
OPENCODE_ANTHROPIC_MODEL=litellm/claude-sonnet-4-5 \
cargo test --test opencode_anthropic_server_live -- --ignored --nocapture
# -> [live] >>> ASSISTANT SAID: Red, yellow, blue.
```

LAP itself needs **no** opencode-specific code — register this server's URL/key
as a `claude_managed_agents` runtime credential and the existing agent/session
flow drives it.

## API reference

Base URL defaults to `http://localhost:8080`. All `/v1/*` calls honor `x-api-key`, `anthropic-version`, and `anthropic-beta: managed-agents-2026-04-01` (the API key is accepted loosely for the demo).

### Create an agent — `POST /v1/agents`

Body: `{name, model, system, description?, tools?, mcp_servers?, permissions?, metadata?}`. `model` is a string (`"anthropic/claude-sonnet-4-5"`) or `{id}`. Returns a durable agent `{id:"agt_...", type:"agent", name, model:{id}, system, version, created_at, ...}`.

```bash
curl -s -X POST "$BASE/v1/agents" \
  -H "content-type: application/json" \
  -H "x-api-key: any-key" \
  -H "anthropic-version: 2023-06-01" \
  -H "anthropic-beta: managed-agents-2026-04-01" \
  -d '{
    "name": "Docs Helper",
    "model": "anthropic/claude-sonnet-4-5",
    "system": "You are a terse documentation assistant.",
    "permissions": { "bash": "ask", "edit": "allow" },
    "mcp_servers": []
  }'
```

`GET /v1/agents` lists agents; `GET /v1/agents/:id` fetches one.

### Create an environment — `POST /v1/environments`

Body: `{name, config?, description?, scope?}` → `{id:"env_...", type:"environment"}`. `config` may carry a workspace `repository`/`ref`.

```bash
curl -s -X POST "$BASE/v1/environments" \
  -H "content-type: application/json" \
  -H "x-api-key: any-key" \
  -d '{
    "name": "docs-env",
    "config": { "repository": "https://github.com/acme/docs", "ref": "main" }
  }'
```

### Create a session — `POST /v1/sessions`

Body: `{agent:"agt_...", environment_id?, title?, metadata?}` → `{id, type:"session", agent, environment_id, status:"running"}`. Provisions opencode for the agent.

```bash
curl -s -X POST "$BASE/v1/sessions" \
  -H "content-type: application/json" \
  -H "x-api-key: any-key" \
  -d '{ "agent": "agt_123", "environment_id": "env_123", "title": "hello" }'
```

### Send events (a prompt) — `POST /v1/sessions/:id/events`

Body: `{events:[{type:"user.message", content:"..." | [{type:"text",text:"..."}]}]}`. Forwards the prompt to opencode and returns `202 Accepted`; the agent's reply arrives on the SSE stream.

```bash
curl -s -X POST "$BASE/v1/sessions/ses_123/events" \
  -H "content-type: application/json" \
  -H "x-api-key: any-key" \
  -d '{ "events": [ { "type": "user.message", "content": [ { "type": "text", "text": "Say hello in 3 words." } ] } ] }'
```

### Stream events — `GET /v1/sessions/:id/events/stream`

Server-Sent Events. Frames are `event: <type>\ndata: <json>\n\n`. Anthropic event types emitted:

| Event | Data |
| --- | --- |
| `agent.message` | `{content:[{type:"text",text}], model}` |
| `agent.thinking` | reasoning delta |
| `agent.tool_use` | tool call |
| `agent.tool_result` | tool result |
| `session.status_running` | session became active |
| `session.status_idle` | turn finished |
| `session.error` | error payload |

```bash
curl -sN "$BASE/v1/sessions/ses_123/events/stream" -H "x-api-key: any-key"
```

`GET /v1/sessions/:id/events` returns the buffered list as `{data:[...]}`.

### Health — `GET /health`

```bash
curl -s "$BASE/health"   # {"ok":true,"opencode":true}
```

## End-to-end (pure curl)

Create an agent → environment → session, open the SSE stream in the background, POST a `user.message`, and watch `agent.message` followed by `session.status_idle`.

```bash
set -euo pipefail
BASE="${BASE:-http://localhost:8080}"
H=(-H "content-type: application/json" -H "x-api-key: any-key")

# 1. create an agent
aid=$(curl -s "${H[@]}" -X POST "$BASE/v1/agents" \
  -d '{"name":"E2E","model":"anthropic/claude-sonnet-4-5","system":"You are terse."}' \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')

# 2. create an environment
eid=$(curl -s "${H[@]}" -X POST "$BASE/v1/environments" \
  -d '{"name":"e2e-env","config":{}}' \
  | python3 -c 'import sys,json;print(json.load(sys.stdin).get("id",""))')

# 3. create a session bound to the agent + environment
sid=$(curl -s "${H[@]}" -X POST "$BASE/v1/sessions" \
  -d "{\"agent\":\"$aid\",\"environment_id\":\"$eid\",\"title\":\"e2e\"}" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')

# 4. open the SSE stream in the background
curl -sN "$BASE/v1/sessions/$sid/events/stream" -H "x-api-key: any-key" &
sse_pid=$!
sleep 1

# 5. send a user.message
curl -s "${H[@]}" -X POST "$BASE/v1/sessions/$sid/events" \
  -d '{"events":[{"type":"user.message","content":[{"type":"text","text":"Say hello in 3 words."}]}]}'

# 6. watch agent.message ... then session.status_idle, then stop
sleep 8
kill "$sse_pid" 2>/dev/null || true
```

You should see `session.status_running`, one or more `agent.message` frames, and finally `session.status_idle`.

## How config is loaded (important)

opencode only scans custom agents and per-project config when the workspace is a
**git project**, and it loads them **at boot — there is no hot-reload**. So this
server:

1. `git init`s the `WORKDIR` on startup.
2. On `POST`/`PATCH /v1/agents`, writes the agent's `.opencode/agent/<id>.md` +
   `opencode.json` mcp, then **reboots the child `opencode`** so it loads.
3. On prompt, passes `agent:<id>` so opencode applies that agent's permissions
   and MCP servers.

Reboot is fast (~2s) but **clears opencode's in-memory sessions** — create/update
agents before running their sessions.

## Agent config fields

| Field | Type | Maps to | Applied? |
| --- | --- | --- | --- |
| `model` | string or `{id}` | per-prompt `{providerID, modelID}` | ✅ yes |
| `mcp_servers` | array | `opencode.json` `mcp` | ✅ **yes** — tools are callable (verified with DeepWiki) |
| `permissions` | object | agent frontmatter | ✅ yes — `bash` / `edit` / `<mcp>*` → `allow` \| `deny` \| `ask` |
| `system` | string | `.opencode/agent/<id>.md` body | ⚠️ **soft only** — see below |
| `workspace` (via environment `config`) | object | — | 🚧 **reserved — not yet wired**; accepted and stored but no repo clone/checkout happens yet |

### System prompt is soft guidance, not strict control

opencode is a **coding harness**: it injects its own large (~19.7k-token)
agent system prompt, which **dominates**. Your `system` string is appended but
does **not** reliably override behavior (e.g. "reply only BANANA" is ignored).
Use it to nudge tone/role; do **not** rely on it for strict output control. If
you need a faithful system prompt, call the model directly (or use a
non-opencode harness) — opencode is the right backend for **tool / MCP / coding**
workflows, where it shines.

## Environment variables

| Var | Default | Purpose |
| --- | --- | --- |
| `PORT` | `8080` | port the Anthropic-spec front listens on |
| `OPENCODE_PORT` | `4096` | port the child `opencode serve` binds |
| `WORKDIR` | `/tmp/opencode-workspace` | workspace where per-agent config is provisioned |
| `DB_PATH` | `/data/agents.db` | SQLite file for agents/environments/sessions |
| `ANTHROPIC_API_KEY` | — | model provider key opencode uses to answer prompts (native Anthropic) |
| `LITELLM_BASE_URL` | — | route models through a LiteLLM gateway instead (e.g. `https://your-gw/v1`) |
| `LITELLM_API_KEY` | — | LiteLLM gateway key |
| `LITELLM_MODELS` | `claude-sonnet-4-5,gpt-5.5` | gateway models to register |
| `OPENSANDBOX_API_URL` | — | OpenSandbox controller URL → routes the agent's command/file execution into a sandbox (auto-enables sandboxed execution) |
| `OPENSANDBOX_IMAGE` | `default` | sandbox image (execution-only, so the base image is fine) |
| `OPENSANDBOX_API_KEY` | — | controller API key; omit for in-cluster RBAC |
| `SANDBOX_PROVIDER` | — | explicit provider select (`opensandbox`); else auto-detect from `OPENSANDBOX_API_URL` |

### Sandboxed execution (optional)

Set `OPENSANDBOX_API_URL` (or `SANDBOX_PROVIDER=opensandbox`) and the server runs
the agent's **commands/file ops in an OpenSandbox sandbox** instead of on this
host: it denies opencode's native `bash`/`edit` and wires a sandbox-exec MCP
(`sandbox_exec` / `sandbox_read_file` / `sandbox_write_file`) backed by **raw HTTP**
calls to the OpenSandbox controller + execd (no SDK dependency). opencode itself
stays here; only execution is isolated.

When `LITELLM_BASE_URL` + `LITELLM_API_KEY` are set, the server configures an
opencode provider `litellm` (via opencode's native Anthropic adapter pointed at
`{LITELLM_BASE_URL}/messages`). Address models as `litellm/<model>`, e.g. an
agent with `"model": "litellm/claude-sonnet-4-5"`.

## Interrupting a turn — `POST /v1/sessions/:id/abort`

Stops the in-flight generation (proxies opencode's session abort):

```bash
curl -X POST $BASE/v1/sessions/$SID/abort -H "x-api-key: k"   # -> {"aborted":true}
```

The event stream stops emitting `agent.message` and settles on
`session.status_idle`.

## Verified scenarios

Driven through the **LAP SDK** (`claude_managed_agents`, only `api_base`/`api_key`
pointed here) against a LiteLLM-gateway-backed server:

- **query → response** — `Name the three primary colors` → streamed
  `agent.message` deltas → `Red, yellow, blue.` → `session.status_idle`.
- **query → interrupt** — a 500-word essay request streamed
  (`# The Rise, Glory, and Fall of the Roman Empire …`), then `POST …/abort`
  → `{"aborted":true}` and **zero further tokens**; the turn ended.
- **MCP tool use** — an agent created with `mcp_servers:[{name:"deepwiki",url:…}]`
  (+ `permissions:{"deepwiki*":"allow"}`) → asked to look up a repo → emitted
  `agent.tool_use` (`deepwiki_read_wiki_structure`) and returned real wiki
  sections for `facebook/react` / `vercel/next.js`.

Reproduce the SDK path with:
`cargo test --test opencode_anthropic_server_live -- --ignored --nocapture`
(set `OPENCODE_ANTHROPIC_BASE` + `OPENCODE_ANTHROPIC_MODEL`).

## Deploy to Render

Deploy as a **Docker web service**:

1. New → Web Service → point at this repo; Render builds the `Dockerfile`.
2. Add a **mounted disk** and set its mount path to the directory of `DB_PATH` (e.g. mount at `/var/data` and set `DB_PATH=/var/data/agents.db`) so the SQLite agent store survives deploys/restarts.
3. Set the **model provider key** env var (`ANTHROPIC_API_KEY=...`) so the child opencode can answer prompts.
4. Health check path: `/health` (returns `{ok:true, opencode:bool}`).

`PORT` is provided by Render; `OPENCODE_PORT` and `WORKDIR` can be left at defaults.

## Closing

This is a **standalone server** the **LAP / lite-harness SDK** talks to via the **Anthropic Managed Agents contract** — the same `claude_managed_agents` runtime, with only `api_base`/`api_key` changed. opencode is purely an implementation detail behind that contract.
