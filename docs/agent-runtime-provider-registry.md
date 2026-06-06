# Agent Runtime Provider Registry Proposal

## Summary

The managed-agents Rust SDK should use an auto-discovered provider registry for
agent runtimes instead of growing provider-specific branches inside
`src/sdk/agents/client.rs`.

The main gateway provider layer already follows this pattern: `build.rs` scans
`src/sdk/providers/<name>/`, generates module wiring, and each provider registers
itself through a small `init(registry)` function. The managed-agents SDK can use
the same shape under `src/sdk/agents/providers/<name>/`.

This keeps `client.rs` as a generic facade and transport owner. Runtime-specific
protocol details live with the runtime provider.

## Problem

`src/sdk/agents/client.rs` currently owns both SDK orchestration and
Anthropic-specific request behavior:

- runtime config extraction from `LapConfig`
- default base URL handling
- auth and beta headers
- request paths for agents, environments, sessions, event send, and event stream
- response ID extraction

That is manageable while `claude_managed_agents` is the only runtime. Once
additional runtimes are added, the natural implementation path is to add
`match AgentRuntime` or `if provider == ...` branches throughout `client.rs`.
That makes every new runtime touch shared SDK orchestration and turns the client
into an expanding provider switchboard.

The repo standards point in the opposite direction: add providers by adding
provider modules, and keep provider-specific behavior in provider-owned code.

## Goals

- Add a managed-agent runtime by adding a new folder under
  `src/sdk/agents/providers/`.
- Keep `client.rs` provider-generic.
- Reuse the existing build-time auto-discovery pattern from `src/sdk/providers`.
- Preserve the current public SDK surface.
- Make provider-specific behavior testable in isolation.
- Fail clearly at boot/client construction when a requested runtime is not
  configured or unsupported.
- Avoid replacing one central branch list with a central runtime enum list.

## Non-Goals

- Runtime plugin loading from the filesystem at process startup.
- Changing the public managed-agents SDK method names.
- Moving HTTP ownership into providers. The shared client should still own the
  `reqwest::Client`; providers should describe requests and transform protocol
  shape.
- Solving Python bindings in the same change.

## Proposed Layout

```text
src/sdk/agents/
├── client.rs
├── events.rs
├── mod.rs
├── providers/
│   ├── mod.rs
│   ├── transform.rs
│   ├── anthropic/
│   │   └── mod.rs
│   └── cursor/
│       └── mod.rs
└── types.rs
```

`build.rs` would generate `agents_providers_generated.rs` from subdirectories
under `src/sdk/agents/providers/`, the same way it currently generates
`providers_generated.rs` from `src/sdk/providers/`.

Each runtime provider module exposes:

```rust
pub fn init(registry: &mut AgentProviderRegistry) {
    registry.register(AnthropicManagedAgentsProvider);
}
```

## Provider Contract

The managed-agents provider trait should separate shared transport from
runtime-specific request and response shape.

```rust
pub trait AgentProvider: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn default_base_url(&self) -> &'static str;

    fn supports_config(&self, config: &LapConfig) -> bool;

    fn extract_runtime_config(
        &self,
        config: &LapConfig,
    ) -> Result<RuntimeConfig, AgentSdkError>;

    fn apply_auth_headers(
        &self,
        request: reqwest::RequestBuilder,
        config: &RuntimeConfig,
    ) -> reqwest::RequestBuilder;

    fn create_agent_request(
        &self,
        params: CreateAgentParams,
    ) -> Result<AgentRequest<Value>, AgentSdkError>;

    fn managed_agent_from_response(
        &self,
        raw: Value,
    ) -> Result<ManagedAgent, AgentSdkError>;

    fn create_environment_request(
        &self,
        params: CreateEnvironmentParams,
    ) -> Result<AgentRequest<Value>, AgentSdkError>;

    fn environment_from_response(
        &self,
        raw: Value,
    ) -> Result<Environment, AgentSdkError>;

    fn create_session_request(
        &self,
        params: CreateSessionParams,
    ) -> Result<AgentRequest<Value>, AgentSdkError>;

    fn session_from_response(
        &self,
        raw: Value,
    ) -> Result<SessionContextUpdate, AgentSdkError>;

    fn send_events_request(
        &self,
        session_id: &str,
        context: &SessionContext,
        params: SendEventsParams,
    ) -> Result<AgentRequest<Value>, AgentSdkError>;

    fn send_events_from_response(
        &self,
        session_id: &str,
        context: &SessionContext,
        raw: Value,
    ) -> Result<SessionEventUpdate, AgentSdkError>;

    fn stream_events_request(
        &self,
        session_id: &str,
        context: &SessionContext,
    ) -> Result<AgentRequest<()>, AgentSdkError>;
}

pub trait StreamProvider: AgentProvider {
    fn normalize_stream(&self, stream: AgentEventStream) -> AgentEventStream;
}

pub enum AgentRequest<B> {
    Http {
        method: Method,
        path: String,
        body: B,
    },
    Synthetic(SyntheticResource),
}

pub enum SyntheticResource {
    Environment(Environment),
    Session(SessionContextUpdate),
}
```

The exact names can change during implementation, but the important boundary is:

- `client.rs` owns HTTP execution, default-runtime resolution, session memory,
  and public SDK resources.
- providers own runtime IDs, auth headers, path/body construction, response
  parsing, synthetic-resource behavior, and optional stream normalization.

Helper return types such as `SessionContextUpdate` and `SessionEventUpdate`
should carry both the public SDK response and any remembered provider state, such
as a provider-local agent ID or run ID.

`supports_config` answers only whether the provider has enough configuration to
participate. `extract_runtime_config` returns the fully usable config for that
provider, or an error if the matching configuration is malformed.

Synthetic resources are explicit request variants, not hidden provider hooks.
This lets `client.rs` decide when no HTTP should be executed, and makes those
paths easy to trace and test.

## What `client.rs` Looks Like

With the registry in place, `client.rs` becomes mostly orchestration:

```rust
pub struct Lap {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    providers: AgentProviderRegistry,
    runtimes: HashMap<AgentRuntimeId, RuntimeConfig>,
    session_contexts: Mutex<HashMap<String, SessionContext>>,
}

impl Lap {
    pub fn new(config: LapConfig) -> Self {
        let mut providers = AgentProviderRegistry::new();
        agent_providers::register_all(&mut providers);

        let runtimes = providers.configured_runtimes(&config);
        Self::with_http(configured_http_client(), providers, runtimes)
    }

    fn request(
        &self,
        runtime: &AgentRuntimeId,
        method: Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, AgentSdkError> {
        let config = self.runtime_config(runtime)?;
        let provider = self.provider(runtime)?;

        let request = self
            .inner
            .http
            .request(method, format!("{}{}", config.base_url, path))
            .header(header::CONTENT_TYPE, "application/json");

        Ok(provider.apply_auth_headers(request, config))
    }

    async fn execute(
        &self,
        runtime: &AgentRuntimeId,
        request: AgentRequest<Value>,
    ) -> Result<Value, AgentSdkError> {
        let AgentRequest::Http { method, path, body } = request else {
            return Err(AgentSdkError::InvalidRequest(
                "synthetic request cannot be executed as HTTP".to_owned(),
            ));
        };

        let response = self
            .request(runtime, method, &path)?
            .json(&body)
            .send()
            .await?;

        response_json(response).await
    }

    async fn execute_stream(
        &self,
        runtime: &AgentRuntimeId,
        request: AgentRequest<()>,
    ) -> Result<AgentEventStream, AgentSdkError> {
        let AgentRequest::Http { method, path, body: () } = request else {
            return Err(AgentSdkError::InvalidRequest(
                "synthetic request cannot be streamed".to_owned(),
            ));
        };

        let response = self
            .request(runtime, method, &path)?
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await?;

        let stream = stream_events(ensure_success(response).await?);
        Ok(match self.stream_provider(runtime)? {
            Some(provider) => provider.normalize_stream(stream),
            None => stream,
        })
    }
}
```

The public resource methods delegate to the selected provider:

```rust
impl Agents<'_> {
    pub async fn create(&self, params: CreateAgentParams) -> Result<ManagedAgent, AgentSdkError> {
        let runtime = params.lap_agent_runtime.clone();
        let provider = self.client.provider(&runtime)?;
        let request = provider.create_agent_request(params)?;
        let raw = self.client.execute(&runtime, request).await?;

        provider.managed_agent_from_response(raw)
    }
}

impl Sessions<'_> {
    pub async fn create(&self, params: CreateSessionParams) -> Result<Session, AgentSdkError> {
        let runtime = self.client.resolve_runtime(params.lap_agent_runtime.clone())?;
        let provider = self.client.provider(&runtime)?;
        let update = match provider.create_session_request(params)? {
            AgentRequest::Http { method, path, body } => {
                let raw = self
                    .client
                    .execute(&runtime, AgentRequest::Http { method, path, body })
                    .await?;

                provider.session_from_response(raw)?
            }
            AgentRequest::Synthetic(SyntheticResource::Session(update)) => update,
            AgentRequest::Synthetic(_) => {
                return Err(AgentSdkError::InvalidRequest(
                    "session create returned the wrong synthetic resource".to_owned(),
                ));
            }
        };

        self.client
            .remember_session_context(&update.session.id, update.context)?;

        Ok(update.session)
    }
}

impl SessionEvents<'_> {
    pub async fn send(
        &self,
        session_id: &str,
        params: SendEventsParams,
    ) -> Result<SendEventsResponse, AgentSdkError> {
        let context = self.client.context_for_session(session_id)?;
        let provider = self.client.provider(&context.runtime)?;
        let request = provider.send_events_request(session_id, &context, params)?;
        let raw = self.client.execute(&context.runtime, request).await?;
        let update = provider.send_events_from_response(session_id, &context, raw)?;

        self.client
            .remember_session_context(session_id, update.context)?;

        Ok(update.response)
    }
}
```

This removes the need for provider-specific branches in the public SDK methods.

## Why a Provider Driver, Not Only a Base Transformer

Request/response transformation is enough for Anthropic-style runtimes, but
other managed-agent runtimes may differ beyond payload shape. Cursor-style
runtimes, for example, may need:

- bearer auth instead of Anthropic headers
- nested IDs in create-agent responses
- synthetic environments or sessions
- a run ID remembered in session context
- event-send calls mapped to run creation
- stream URLs based on agent ID and run ID
- stream event normalization

Those behaviors are still provider-specific and should stay out of `client.rs`.
The provider contract should keep the Anthropic-shaped request/response flow
straightforward, while companion traits and explicit synthetic request variants
leave room for unusual runtimes.

## Runtime IDs

To avoid moving the ever-expanding list from `client.rs` into `types.rs`,
`AgentRuntime` should eventually become string-backed instead of a closed enum.
For example:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentRuntimeId(String);

impl AgentRuntimeId {
    pub const CLAUDE_MANAGED_AGENTS: &'static str = "claude_managed_agents";

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

For compatibility, the SDK can keep convenience constructors or constants:

```rust
impl AgentRuntimeId {
    pub fn claude_managed_agents() -> Self {
        Self::new(Self::CLAUDE_MANAGED_AGENTS)
    }
}
```

That gives us provider discovery without requiring a source edit every time a
runtime ID is added. If preserving the enum is more important in the short term,
the registry still removes the branching from `client.rs`, but adding a runtime
would still require one central enum update.

## Migration Plan

1. Add `src/sdk/agents/providers/transform.rs` with
   `AgentProviderRegistry`, `AgentProvider`, and request/context helper types.
2. Add `src/sdk/agents/providers/anthropic/mod.rs` that preserves the current
   `claude_managed_agents` behavior.
3. Extend `build.rs` to generate managed-agent provider modules and
   `register_all`.
4. Convert `AgentRuntime` to a string-backed runtime ID, or add a compatibility
   adapter while the public API migrates.
5. Refactor `client.rs` to resolve providers from the registry and delegate
   provider-specific request/response behavior.
6. Keep existing managed-agent SDK tests passing unchanged.
7. Add focused tests for registry discovery, unsupported runtime errors, and
   provider-specific request construction.
8. Add future runtime providers only as new provider folders.

## Review Questions

- Is the string-backed `AgentRuntimeId` worth doing in the first registry PR, or
  should it be a follow-up migration after the provider trait lands?
- Should provider config extraction stay on `LapConfig`, or should `LapConfig`
  move toward a map keyed by runtime ID?
- Should optional provider extensions use companion traits like
  `StreamProvider`, or should the registry store explicit extension metadata?
- Are explicit `AgentRequest::Synthetic` variants enough for provider-local
  sessions and environments, or should each resource have its own action enum?
