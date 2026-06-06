# Agent Runtime SDK

`src/sdk` is the public Rust client surface for calling managed-agent runtimes
and routing LLM gateway requests through one SDK namespace.

It exposes:

- `Lap` and `LapConfig` for configuring runtime credentials.
- `client.beta().agents()`, `environments()`, and `sessions()` resource handles.
- Normalized session event streaming across supported runtimes.
- Typed event views via `AgentEvent::kind()` and `AgentEvent::payload()`.

Runtime-specific request shapes live behind internal adapters in
`src/sdk/agents/runtimes/`. Adding another runtime should add an adapter there
instead of adding new `match AgentRuntime` branches throughout the SDK resource
layer.

Gateway model routing and LLM provider transformation live under `src/sdk/llms/`.
`sdk::router` and `sdk::providers` remain compatibility re-exports, while new
gateway code can import the canonical modules from `sdk::llms`.
