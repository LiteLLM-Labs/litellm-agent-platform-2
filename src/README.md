# src/

Source layout for the litellm-rust gateway. A request flows
`endpoint → router → transformation → llm api`; see
[docs/architecture.md](../docs/architecture.md) for the full picture.

The codebase splits request routing, translation, managed-agent runtime SDK,
and proxy concerns into separate folders. SDK routing and translation live under
`sdk/`, provider-specific implementations live under `sdk/providers/`, and the
proxy server wraps them with config, auth, state, and HTTP routes.

## Entrypoints

| File | What it does |
|---|---|
| `main.rs` | Binary entry. Parses args, loads config, builds the provider registry + router, starts the server. Also dispatches the `claude` CLI wizard. |
| `lib.rs` | Crate root. Declares the public modules below. |
| `errors.rs` | `GatewayError` — the shared error type, mapped to HTTP responses in one place. Used by both halves, so it lives at the top level. |

## Folders

| Folder | Responsibility |
|---|---|
| `sdk/routing/` | **Routing.** Request/model routing above LLM and runtime translation. |
| `sdk/translation/` | **Translation traits.** Shared traits/registries for LLM request transformation and runtime adapters. |
| `sdk/providers/` | **Provider integrations.** Each provider owns its supported capabilities: `llm/` for request transformation, `runtime/` for managed-agent adapters. |
| `sdk/agents/` | **Agent Runtime SDK.** The `Lap` client, public runtime resource types, and normalized events. |
| `proxy/` | **Proxy-server concerns**, kept out of the SDK: `config.rs` (`config.yaml` parse + env expansion + validation), `state.rs` (`AppState` — config, router, shared HTTP client), `auth/` (master-key check). |
| `http/` | HTTP layer. Routes (`routes.rs`), the `/v1/messages` endpoint (`messages.rs`), health check, and `llm.rs` — the **only** place that does outbound networking to providers. |
| `cli/` | The `litellm-rust claude` wizard: configures Claude Code to point at the gateway (arg parsing, credential storage, terminal prompts). |

## Adding a provider

Drop a provider folder under `sdk/providers/<name>/` with a `llm/mod.rs`
(`pub fn init`) and a `llm/transformation.rs`. `build.rs` wires it in
automatically. See [docs/architecture.md](../docs/architecture.md#providers-are-self-contained).
