---
name: provider-integration
description: Use when adding or changing a managed-agents SDK provider integration in this repo, including new runtime providers under src/sdk/agents/providers, supported-params contracts, provider transforms, docs, and tests.
---

# Provider Integration

Use this skill for managed-agents SDK provider work in this repo.

## Ground Rules

- Read `AGENTS.md` and `CODING_STANDARDS.md` before implementation changes.
- Keep the public SDK Anthropic-shaped. Add `lap_` fields only for LAP-owned
  routing/config choices.
- Do not add DB, vault, idempotency, or credential-storage logic to the SDK.
  LAP orchestration owns those concerns before calling the provider SDK.
- Do not add provider-specific JSON escape hatches such as session `resources`.
  If a provider field does not translate from the strict contract, leave it out
  and document it as unsupported.
- Returned IDs are provider/runtime IDs, not LAP database IDs.

## File Layout

Follow the existing layout:

```text
src/sdk/agents/
  client.rs
  events.rs
  mod.rs
  types.rs
  providers/
    mod.rs
    transform.rs
    <provider>/
      mod.rs
      transformation.rs
```

Provider folders should be small and own endpoint mapping, request transforms,
response ID extraction, and event normalization for that runtime.

## Provider Contract

Each provider must implement the base trait in
`src/sdk/agents/providers/transform.rs`.

For every supported endpoint, expose both:

```rust
supported_managed_agents_<endpoint>_params(...)
transform_managed_agents_<endpoint>_params(...)
```

The supported-params function is the source of truth for what translates. The
transform function must return the exact JSON body that will be sent upstream.

Use strict request structs from `src/sdk/agents/types.rs`. If a new Anthropic
managed-agents field is needed, add a typed field there; do not use
`serde_json::Value` unless the upstream contract is intentionally unstructured.

## Implementation Workflow

1. Add or update typed request structs in `src/sdk/agents/types.rs`.
2. Register runtime config in `LapConfig` and `AgentRuntime`.
3. Add provider folder under `src/sdk/agents/providers/<provider>/`.
4. Implement supported-param lists and transform functions first.
5. Wire network calls to call the transform functions, not duplicate mapping.
6. Normalize provider events into SDK events such as `agent.message`,
   `agent.tool_use`, `session.status_idle`, and `session.error`.
7. Expose user-facing docs under `docs/`.

## Required Tests

Add provider unit tests in the provider `transformation.rs` module:

- supported params for every endpoint
- exact transformed JSON for every endpoint
- unsupported fields are omitted or rejected clearly
- provider-specific event normalization

Add SDK integration tests under `tests/` using mock HTTP:

- provider auth headers
- expected paths
- request body shape
- stream parsing through `client.beta().sessions().events().stream(...)`

If a live provider test is useful, make it ignored and gated by an env var:

```rust
#[tokio::test]
#[ignore = "requires PROVIDER_API_KEY and creates real provider resources"]
async fn provider_live_smoke() { ... }
```

Never commit real API keys.

## Verification

Run the contract checks before committing:

```bash
cargo fmt --all --check
cargo test --test managed_agents_sdk --locked
cargo test sdk::agents::providers --locked
cargo check --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

If `scripts/check_code_size.py` fails on unrelated existing files, state that
explicitly and do not hide it by changing thresholds.
