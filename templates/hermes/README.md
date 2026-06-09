# Hermes Agent Runtime Harness

Exposes [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) behind the
**Anthropic Managed Agents API spec** so any LAP SDK client can drive it without code changes.

```
┌─────────────────────────────────────────────────────────┐
│  LAP SDK / UI / Slack bot                               │
│    POST /v1/sessions/:id/events                         │
│    GET  /v1/sessions/:id/events/stream  (SSE)           │
└──────────────────────┬──────────────────────────────────┘
                       │ HTTP (localhost:8080)
┌──────────────────────▼──────────────────────────────────┐
│  templates/hermes  (Node.js Express)                    │
│  • in-memory agent/session/env store                    │
│  • hermesRunTurn: spawns hermes CLI, streams stdout     │
└──────────────────────┬──────────────────────────────────┘
                       │ OpenAI-compatible HTTP
┌──────────────────────▼──────────────────────────────────┐
│  LiteLLM Gateway  (LITELLM_API_BASE)                    │
│  → any model provider                                   │
└─────────────────────────────────────────────────────────┘
```

## Quickstart (Docker)

```bash
docker build -t hermes-server templates/hermes/
docker run --rm -p 8080:8080 \
  -e LITELLM_API_BASE=http://host.docker.internal:4000/v1 \
  -e LITELLM_API_KEY=sk-... \
  -e LITELLM_DEFAULT_MODEL=claude-sonnet-4-6 \
  hermes-server
```

## Quickstart (local)

```bash
cd templates/hermes
npm install
LITELLM_API_BASE=http://localhost:4000/v1 LITELLM_API_KEY=sk-... node src/index.mjs
```

## End-to-end curl example

```bash
BASE=http://localhost:8080

# 1. Create an agent
AGENT=$(curl -sS -X POST $BASE/v1/agents \
  -H "content-type: application/json" \
  -d '{"name":"my-agent","model":"claude-sonnet-4-6","system":"You are helpful."}')
AGENT_ID=$(echo $AGENT | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s).id))")

# 2. Create a session
SESSION=$(curl -sS -X POST $BASE/v1/sessions \
  -H "content-type: application/json" \
  -d "{\"agent_id\":\"$AGENT_ID\"}")
SESSION_ID=$(echo $SESSION | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s).id))")

# 3. Send a prompt (fire-and-forget, 202)
curl -sS -X POST $BASE/v1/sessions/$SESSION_ID/events \
  -H "content-type: application/json" \
  -d '{"content":"What is 2 + 2?"}' -o /dev/null

# 4. Stream the response
curl -sS --max-time 30 $BASE/v1/sessions/$SESSION_ID/events/stream
```

## LAP SDK snippet (Rust)

```rust
use lap_sdk::{Lap, LapConfig, AgentRuntime};

let config = LapConfig::hermes("http://localhost:8080");
let lap = Lap::new(config);
let agent = lap.agents().create(CreateAgentParams {
    lap_agent_runtime: AgentRuntime::Hermes,
    name: "my-agent".into(),
    model: "claude-sonnet-4-6".into(),
    system: "You are helpful.".into(),
    ..Default::default()
}).await?;
```

## Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `LITELLM_API_BASE` | Yes | — | LiteLLM gateway base URL (e.g. `http://localhost:4000/v1`) |
| `LITELLM_API_KEY` | Yes | — | LiteLLM API key |
| `LITELLM_DEFAULT_MODEL` | No | `claude-sonnet-4-6` | Default model ID |
| `PORT` | No | `8080` | Server port |

## API routes

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | Liveness check |
| `POST` | `/v1/agents` | Create agent |
| `GET` | `/v1/agents` | List agents |
| `GET` | `/v1/agents/:id` | Get agent |
| `PATCH` | `/v1/agents/:id` | Update agent |
| `POST` | `/v1/environments` | Create environment |
| `POST` | `/v1/sessions` | Create session |
| `POST` | `/v1/sessions/:id/events` | Send prompt (202) |
| `POST` | `/v1/sessions/:id/abort` | Abort active turn |
| `GET` | `/v1/sessions/:id/events` | List events (stub) |
| `GET` | `/v1/sessions/:id/events/stream` | Live SSE stream |

## SSE event shapes

```
data: {"id":"evt_...","type":"agent.message","properties":{"sessionID":"ses_...","content":[{"type":"text","text":"Hello"}]}}
data: {"id":"evt_...","type":"message.part.delta","properties":{"sessionID":"ses_...","messageID":"msg_...","field":"text","delta":"..."}}
data: {"id":"evt_...","type":"session.status_idle","properties":{"sessionID":"ses_...","stop_reason":{"type":"end_turn"}}}
```
