# Design: Multiple Runtime Harnesses with Aliases (v2)

## Problem

Gateway has 3 hardcoded runtimes (`claude_managed_agents`, `cursor`, `opencode`). Users need additional harness endpoints (e.g. staging Anthropic, team-specific Cursor) addressable by alias.

## Goals

- Admin registers custom harnesses with aliases; platform users reference by alias
- Zero breaking changes — existing runtime names work unchanged
- Alias survives full session lifecycle: creation, follow-up prompts, event streaming

## Non-Goals

- Per-user harness scoping (global/admin-only for now)

---

## Core: `ResolvedRuntime`

Single resolver used by every session code path — eliminates per-call registry lookups:

```rust
pub(crate) struct ResolvedRuntime {
    pub alias: String,           // stored in DB as session.runtime
    pub agent_runtime: AgentRuntime,  // enum from api_spec or direct static match
    pub credential: RuntimeCredential,
    pub adapter: Arc<dyn RuntimeAdapter>,
}

pub(crate) async fn resolve_runtime(
    pool: &PgPool, state: &AppState, alias: &str,
) -> Result<ResolvedRuntime, GatewayError> {
    // 1. Static registry (claude_managed_agents, cursor, opencode) → unchanged path
    // 2. DB lookup by alias → api_spec maps to existing adapter
}
```

All `sdk_runtime(runtime)` and `runtime_registry().entry_for_id(runtime)` call sites replaced with `resolved.agent_runtime` / `resolved.adapter`.

---

## Data Model

```sql
CREATE TABLE IF NOT EXISTS "LiteLLM_RuntimeHarnessTable" (
  id          TEXT PRIMARY KEY,
  alias       TEXT UNIQUE NOT NULL,
  api_spec    TEXT NOT NULL CHECK (api_spec IN ('claude_managed_agents', 'cursor', 'opencode')),
  api_base    TEXT NOT NULL,
  created_at  BIGINT NOT NULL,
  updated_at  BIGINT NOT NULL
);
```

API key: `LiteLLM_CredentialsTable`, `credential_name = 'runtime-harness:{alias}'`, `scope = 'global'`, encrypted via `credential_crypto`.

---

## API

Keep `/api/agent-runtimes` intact. Add:

| Method | Path | Notes |
|--------|------|-------|
| GET | `/api/runtime-harnesses` | defaults (`is_default: true`) + custom DB rows |
| POST | `/api/runtime-harnesses` | `{ alias, api_spec, api_base, api_key }` |
| PUT | `/api/runtime-harnesses/{alias}` | update credentials |
| DELETE | `/api/runtime-harnesses/{alias}` | custom only |

All write operations: master key required, atomic (harness row + credential in sync), reserved/non-slug alias rejected.

Reserved aliases: `claude_managed_agents`, `cursor`, `opencode`, `claude_agents`.  
Valid slug: `[a-zA-Z0-9_-]+`.

---

## Frontend

- `AgentRuntimeId` type widened from 3-value union → `string`
- `isAgentRuntimeId()` in `sessions/page.tsx` accepts any non-empty string
- `createSession`, `sendMessageWithRuntimeModel` in `api.ts` accept `runtime?: string`
- `/runtimes` page: unified list (defaults + custom) with "+ New Runtime" modal
- Agent creation: runtime selector dropdown from `/api/runtime-harnesses`

---

## Tests

Backend integration tests: create harness, session via alias, follow-up prompt, list/stream events, delete, reject reserved/invalid aliases.
