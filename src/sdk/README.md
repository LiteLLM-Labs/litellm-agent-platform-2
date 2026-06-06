# Agent Runtime SDK

`src/sdk` is the public Rust client surface for calling managed-agent runtimes
and routing LLM gateway requests through one SDK namespace.

It exposes:

- `Lap` and `LapConfig` for configuring runtime credentials.
- `client.beta().agents()`, `environments()`, and `sessions()` resource handles.
- Normalized session event streaming across supported runtimes.
- Typed event views via `AgentEvent::kind()` and `AgentEvent::payload()`.

Runtime-specific request shapes live behind provider-owned adapters in
`src/sdk/providers/<provider>/runtime/`. Adding another runtime should add an
adapter there and register it in `src/sdk/providers/runtime.rs` instead of
adding new `match AgentRuntime` branches throughout the SDK resource layer.

Gateway model routing lives under `src/sdk/llms/`; provider-owned LLM
transformations live under `src/sdk/providers/<provider>/llm/`.
