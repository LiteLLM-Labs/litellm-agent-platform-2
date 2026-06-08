# Design: Multiple Runtime Harnesses with Aliases

## Problem

Gateway has 3 hardcoded runtimes (`claude_managed_agents`, `cursor`, `opencode`). Users need to add additional harness endpoints — e.g. a staging Anthropic endpoint, a team-specific Cursor instance — each addressable by a distinct alias.

## Goals

- Let gateway admins register multiple harnesses (any API spec) with custom aliases
- Platform users reference harnesses by alias when building agents
- Zero breaking changes — existing runtime names continue to work

## Non-Goals

- Per-user harness scoping (global only for now)
- Harness config file export/import

## Data Model

New table `LiteLLM_RuntimeHarnessTable`:

```sql
CREATE TABLE IF NOT EXISTS "LiteLLM_RuntimeHarnessTable" (
  id          TEXT PRIMARY KEY,
  alias       TEXT UNIQUE NOT NULL,   -- user-facing; reserved names rejected
  api_spec    TEXT NOT NULL,          -- "claude_managed_agents" | "cursor" | "opencode"
  api_base    TEXT NOT NULL,
  created_at  BIGINT NOT NULL,
  updated_at  BIGINT NOT NULL
);
```

API keys in existing `LiteLLM_CredentialsTable` under `credential_name = 'runtime-harness:{alias}'`.

## API

Keep `/api/agent-runtimes` intact. Add:

| Method | Path |
|--------|------|
| GET | `/api/runtime-harnesses` |
| POST | `/api/runtime-harnesses` |
| PUT | `/api/runtime-harnesses/{alias}` |
| DELETE | `/api/runtime-harnesses/{alias}` |

GET returns defaults (hardcoded, `is_default: true`) merged with custom DB entries.

## Session Provisioning

`runtime_provision.rs` resolution order:
1. Static match (`claude_managed_agents`, `cursor`, `opencode`) → existing unchanged
2. DB lookup by alias → dispatch on `api_spec`
3. Unknown → 400

## UI Changes

- `/runtimes` page: unified list with "+ New Runtime" modal (alias, api_spec, api_base, api_key)
- Agent creation: runtime selector dropdown (all connected harnesses, default = `claude_managed_agents`)
