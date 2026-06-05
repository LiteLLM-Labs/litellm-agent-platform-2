# Architecture

litellm-rust is a low-overhead gateway. A request flows through four layers:

```
                    ┌─────────────────────────────────────────────┐
  POST /v1/messages │  litellm-rust                                │
  ────────────────► │                                             │
                    │  endpoint ─► router ─► transformation ─► llm │ ──► provider API
  ◄──────────────── │                                             │ ◄──
       response     └─────────────────────────────────────────────┘
```

| Layer | File | Responsibility |
|---|---|---|
| **Endpoint** | `http/messages.rs` | Receive the request, authenticate (master key) |
| **Router** | `providers/router.rs` | Map the public model name to an upstream deployment + provider handler |
| **Transformation** | `providers/<name>/transformation.rs` | Translate the request into the provider's API shape |
| **LLM API** | `http/llm.rs` | The only place that does outbound networking |

## Two halves

The code is split so the translation logic can ship as a standalone SDK,
independent of the proxy server around it:

| Half | Folders | What it is |
|---|---|---|
| **Translation layer** (future SDK) | `providers/` | Provider handlers + the router that picks one. Pure request/response shaping — no auth, no server state. |
| **Proxy server** | `proxy/`, `http/`, `cli/` | Everything around the translation: config loading, master-key auth, shared `AppState`, HTTP endpoints, the CLI wizard. |

`errors.rs` (the shared `GatewayError`) sits at the crate root — both halves use
it. The rule: `providers/` must not depend on `proxy/`. (One bridge remains:
`router.rs::from_config` reads `proxy::config::GatewayConfig`; when the SDK is
extracted, the proxy will build the route table and hand `providers/` plain
data instead.)

## Request flow

A request like:

```bash
curl http://localhost:4000/v1/messages \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer $LITELLM_MASTER_KEY" \
  -d '{"model": "claude-opus-4-6", "messages": [...]}'
```

1. **Endpoint** (`http/messages.rs`) — `proxy::auth` checks the `Authorization: Bearer` token against the configured master key, then parses the body and reads `model`.
2. **Router** (`providers/router.rs`) — looks up `"claude-opus-4-6"` in the route table built at boot from `config.yaml`. Returns a `Route` = `{ deployment, handler }`.
3. **Transformation** (`providers/anthropic/transformation.rs`) — rewrites the model alias to the real upstream name, builds outbound headers (`x-api-key`, `anthropic-version`).
4. **LLM API** (`http/llm.rs`) — sends to `https://api.anthropic.com/v1/messages`, streams the response back byte-for-byte.

## Config → routes

Config types, parsing, env expansion, and boot-time validation all live in
`proxy/config.rs`. The route table comes from `config.yaml`:

```yaml
model_list:
  - model_name: claude-opus-4-6           # ← public name, the lookup key
    litellm_params:
      model: anthropic/claude-opus-4-6     # ← provider_id / upstream_model
      api_key: os.environ/ANTHROPIC_API_KEY
```

At boot, each entry becomes a `Deployment`:

```
provider_id:    "anthropic"
upstream_model: "claude-opus-4-6"
api_base:       "https://api.anthropic.com"   # provider default, or api_base override
api_key:        "sk-ant-..."
```

This is what separates the public alias from the real upstream call.

## Config-defined agents

`config.yaml` can also define agents under `agents`, with sandbox selection and
E2B parameters configured under `general_settings`. Config parsing and
validation still flow through `proxy/config.rs`; agent-specific config types
live in `src/agents/config.rs`.

Agent runs are HTTP-triggered and streamed per run:

```bash
POST /api/agents/{agent_id}/run
GET  /events
```

The run endpoint returns `202` with an `event_url`. The `/events` endpoint is
the SSE stream for agent runs and emits `agent.run.started`,
`agent.sandbox.created`, `agent.output.delta`, and terminal run events. Event
payloads include `agent_id` and `run_id` so clients can filter the stream.

Sandbox provisioning is owned by the proxy. The agent does not receive a
sandbox provisioning tool; the selected sandbox provider creates the sandbox,
starts the harness process, streams process output, and terminates the sandbox
when the run ends.

## Sandbox providers

`general_settings.sandbox_choice` picks the runtime that executes a harness.
Each provider lives in its own module under `src/agents/sandboxes/` and
implements the same `create → start_command → terminate` contract behind
`SandboxRunner`:

| Provider | Module | Backing runtime |
|---|---|---|
| `e2b` | `sandboxes/e2b.rs` | E2B Firecracker microVMs |
| `opensandbox` | `sandboxes/opensandbox.rs` | [OpenSandbox](https://github.com/alibaba/OpenSandbox) Docker/Kubernetes sandboxes |
| `server` (fallback) | `sandboxes/local.rs` | Local process on the gateway host |

The OpenSandbox provider talks to two of its HTTP APIs:

1. **Lifecycle API** (`{api_base}/sandboxes`) — `POST` to create a sandbox from
   an image, poll `GET /sandboxes/{id}` until it reaches `Running`, then
   `GET /sandboxes/{id}/endpoints/{execd_port}` to discover the execution daemon
   URL (plus any secured-access headers), and `DELETE` to tear it down.
2. **Execd API** (`{endpoint}/command`) — `POST` the harness command and decode
   the streamed `ServerStreamEvent` frames (`stdout`/`stderr`) into the gateway's
   `AgentOutputChunk` stream.

## Harnesses

A harness turns an agent definition + prompt into a shell command to run in the
sandbox and maps the command's output into liteharness events. Harnesses live
under `src/agents/harnesses/` and are selected per agent via
`AgentDefinition.harness`:

| Harness | Module | In-sandbox command |
|---|---|---|
| `claude-code` (default) | `harnesses/claude_code.rs` | Claude Agent SDK driver script (structured stream events) |
| `opencode` | `harnesses/opencode.rs` | `opencode run` (plain-text reply) |
| `codex` | `harnesses/codex.rs` | `codex exec --output-last-message` (plain-text reply) |

`opencode` and `codex` share the `plain_text` event mapping: stdout is forwarded
verbatim as `message.part.delta` text so the event stream looks identical
regardless of which CLI produced the answer.

## Providers are self-contained

Each provider is one folder under `src/providers/`. `build.rs` scans for any
subdirectory with a `mod.rs` and wires it in automatically — no edits anywhere
else in the tree.

To add a provider (e.g. OpenAI):

```
src/providers/openai/
├── mod.rs              # pub fn init(registry) { registry.register("openai", ...) }
└── transformation.rs   # impl Transformation
```

That's the whole contract. The router, endpoint, and networking layers never
change.

**Rule:** providers translate protocol shape only. They never make network
calls — all outbound HTTP lives in `http/llm.rs`. This keeps the hot path in one
place and stops each provider from re-implementing (and mis-implementing)
networking.

## Boot sequence

`main.rs` → `serve_gateway`:

1. Load + validate `config.yaml` (`proxy::config::load_config`)
2. Build the `ProviderRegistry` (`register_all`, generated by `build.rs`)
3. Build the `Router` from config + registry
4. Assemble `AppState` (config, router, one shared HTTP client)
5. Start the Axum server

`main.rs` also dispatches the `claude` CLI wizard and `logout` before serving
(see `cli/`).
