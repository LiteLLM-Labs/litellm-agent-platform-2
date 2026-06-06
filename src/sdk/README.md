# Agent Runtime SDK

`src/sdk` is the public Rust client surface for request routing and provider
translation across LLM calls and managed-agent runtimes.

It exposes:

- `Lap` and `LapConfig` for configuring runtime credentials.
- `client.beta().agents()`, `environments()`, and `sessions()` resource handles.
- Normalized session event streaming across supported runtimes.
- Typed event views via `AgentEvent::kind()` and `AgentEvent::payload()`.

Runtime-specific request shapes live behind provider-owned adapters in
`src/sdk/providers/<provider>/runtime/`. Adding another runtime should add an
adapter there and register it in `src/sdk/translation/runtime.rs` instead of
adding new `match AgentRuntime` branches throughout the SDK resource layer.

Model routing lives under `src/sdk/routing/`; shared translation traits live
under `src/sdk/translation/`; provider-owned implementations live under
`src/sdk/providers/<provider>/`.
