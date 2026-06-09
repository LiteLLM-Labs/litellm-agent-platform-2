# Hermes Agent Runtime Harness

Wraps [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) behind the
**Anthropic Managed Agents API spec** so any LAP SDK client can drive it.

```
LAP SDK / UI / Slack
  │  POST /v1/sessions/:id/events
  │  GET  /v1/sessions/:id/events/stream (SSE)
  ▼
templates/hermes  (Node.js Express, port 8080)
  │  hermes chat --provider openai-api --model $MODEL -q "..."
  ▼
LiteLLM Gateway (LITELLM_API_BASE)
  ▼
model provider
```

## Quickstart (Docker)

```bash
docker build -t hermes-server templates/hermes/
docker run --rm -p 8080:8080 \
  -e LITELLM_API_BASE=http://host.docker.internal:4000/v1 \
  -e LITELLM_API_KEY=sk-... \
  hermes-server
```

## Quickstart (local)

```bash
cd templates/hermes
npm install
LITELLM_API_BASE=http://localhost:4000/v1 LITELLM_API_KEY=sk-... node src/index.mjs
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `LITELLM_API_BASE` | — | LiteLLM gateway base URL (required) |
| `LITELLM_API_KEY` | — | LiteLLM API key (required) |
| `LITELLM_DEFAULT_MODEL` | `claude-sonnet-4-6` | Default model ID |
| `PORT` | `8080` | Server port |

## API routes

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | Liveness check → `{"ok":true,"hermes":true}` |
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

## SSE event format

Events are emitted as `event: <type>\ndata: <json>\n\n`:

```
event: agent.message
data: {"content":[{"type":"text","text":"Hello!"}],"model":"claude-sonnet-4-6","stop_reason":null}

event: session.status_idle
data: {"stop_reason":{"type":"end_turn"}}
```

## Smoke test

```bash
BASE=http://localhost:8080 MODEL=claude-sonnet-4-6 templates/hermes/scripts/smoke.sh
```
