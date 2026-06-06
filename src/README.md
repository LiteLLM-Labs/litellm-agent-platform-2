# src/

Source layout for the litellm-rust gateway. A request flows
`endpoint → router → transformation → llm api`; see
[docs/architecture.md](../docs/architecture.md) for the full picture.

The codebase splits LLM translation, managed-agent runtime SDK, and proxy
concerns into separate folders under `sdk/`. LLM provider transformations live
under `sdk/llms/`, the public runtime client lives under `sdk/agents/`, and the
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
| `sdk/llms/` | **LLM translation layer.** One subfolder per LLM provider, auto-discovered at build time. `router.rs` maps model names → upstream deployment + handler; `providers/transform.rs` defines the transformation trait + registry. Providers translate request shape only — never make network calls. |
| `sdk/agents/` | **Agent Runtime SDK.** The `Lap` client, public runtime resource types, normalized events, and internal runtime adapters. |
| `proxy/` | **Proxy-server concerns**, kept out of the SDK: `config.rs` (`config.yaml` parse + env expansion + validation), `state.rs` (`AppState` — config, router, shared HTTP client), `auth/` (master-key check). |
| `http/` | HTTP layer. Routes (`routes.rs`), the `/v1/messages` endpoint (`messages.rs`), health check, and `llm.rs` — the **only** place that does outbound networking to providers. |
| `cli/` | The `litellm-rust claude` wizard: configures Claude Code to point at the gateway (arg parsing, credential storage, terminal prompts). |

## Adding a provider

Drop a folder under `sdk/llms/providers/<name>/` with a `mod.rs` (`pub fn init`) and a
`transformation.rs`. `build.rs` wires it in automatically — no edits anywhere
else. See [docs/architecture.md](../docs/architecture.md#providers-are-self-contained).
