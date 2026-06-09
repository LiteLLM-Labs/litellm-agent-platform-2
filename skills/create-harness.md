---
name: create-harness
description: >
  Scaffold a new harness integration end-to-end — interview the user about the
  target AI agent runtime (e.g. Hermes, Aider, Goose), scaffold harnesses/<name>/
  and templates/<name>/, wire entrypoint, Dockerfile, MCP config, and produce a
  deployable harness + PR-ready template.
  Use when the user says "create a harness for X", "add X as a harness",
  "integrate X into lite-harness", or "build a Hermes/Aider/Goose harness".
---

# Create Harness Integration

Scaffold a complete harness integration for a new AI agent runtime. Produces:
- `harnesses/<name>/` — entrypoint, MCP servers, package.json
- `templates/<name>/` in `litellm-agent-platform-2` — Dockerfile, src/, README, docs/

---

## Step 1: Interview

Collect the following. Ask only what isn't obvious from context.

| Question | Why it matters |
|----------|---------------|
| **Harness name** (slug, e.g. `hermes`) | Directory names, env vars, log prefixes |
| **What is it?** (one sentence) | README intro, template description |
| **How does the agent run?** — CLI command, long-running server, or API? | Determines entrypoint pattern |
| **How does it receive prompts?** — stdin, HTTP endpoint, WebSocket, file? | Determines how the harness feeds work in |
| **How does it stream output?** — SSE, stdout, WebSocket, polling? | Determines how the harness reads results |
| **Does it have a native config file?** (e.g. `opencode.json`, `.aider.conf`) | Determines what to write at boot |
| **Provider/model wiring** — does it use its own model config, or can we inject a LiteLLM gateway? | Determines provider config block |
| **MCP support?** — can it load MCP servers at boot? | Determines whether to wire sandbox + platform MCPs |
| **Auth / env vars required** — what must be set for it to run? | Entrypoint validation + README |
| **Repo with existing Docker setup?** (URL, optional) | Starting point for Dockerfile |

Keep it short. If the user already described the harness, infer what you can.

---

## Step 2: Research the Runtime

Before writing any code, read the runtime's docs/source to learn:

1. **CLI signature** — exact startup command and flags
2. **Config format** — does it read a JSON/YAML/TOML file? What keys matter?
3. **Session/prompt API** — what HTTP endpoint or stdin protocol accepts work?
4. **Streaming API** — how to consume output token-by-token or chunk-by-chunk
5. **Model config** — how does it route to a provider? What env vars does it accept?
6. **MCP loading** — if supported, what config key enables external MCP servers?

Check: official docs, GitHub README, Dockerfile examples in the wild, and any `AGENTS.md` in this repo that references the runtime.

Summarize findings and confirm with the user before scaffolding.

---

## Step 3: Scaffold `harnesses/<name>/`

Create the harness directory. Use the opencode harness at `harnesses/opencode/` as the reference — copy the structure, replace the runtime-specific parts.

### `harnesses/<name>/entrypoint.sh`

Every harness entrypoint follows this pattern:

```bash
#!/usr/bin/env bash
set -euo pipefail
. /opt/lap/common.sh          # loads vault, clones repo, sets REPO_DIR, PORT, etc.

# 1. Normalize LITELLM_API_BASE → BASE (strip trailing slash, ensure /v1)
BASE="${LITELLM_API_BASE%/}"
case "$BASE" in */v1) ;; *) BASE="${BASE}/v1" ;; esac

cd "$REPO_DIR"

# 2. Fetch available models from gateway (required: opencode rejects unknown modelIDs)
MODELS_JSON=$(
  curl -fsS --max-time 10 -H "Authorization: Bearer ${LITELLM_API_KEY}" "${BASE}/models" 2>/dev/null \
    | jq -c '[ .data[].id ] | unique | map({ (.): {} }) | add // {}' 2>/dev/null \
    || printf '%s' '{}'
)
[ -n "$MODELS_JSON" ] || MODELS_JSON='{}'
BOOT_MODEL=$(printf '%s' "$MODELS_JSON" | jq -r 'keys[0] // ""')
[ -z "$BOOT_MODEL" ] && { echo "[entrypoint] FATAL: no models from gateway" >&2; exit 1; }

# 3. Generate MCP config (sandbox, memory, platform tools)
MCP_OBJ=$(node /opt/lap/<name>-mcp/gen-mcp-config.mjs 2>/tmp/gen-mcp.err || echo '{}')
[ -z "$MCP_OBJ" ] && MCP_OBJ='{}'

# 4. Write runtime config file (replace with runtime-specific format)
cat > <runtime-config-file> << EOF
<runtime config with $BASE, $LITELLM_API_KEY, $MODELS_JSON, $MCP_OBJ substituted>
EOF

# 5. Write agent prompt file if AGENT_PROMPT is set
if [ -n "${AGENT_PROMPT:-}" ]; then
  <write to runtime's agent/system prompt location>
fi

echo "[entrypoint] base=${BASE} boot_model=${BOOT_MODEL}"
exec <runtime-start-command> --port "$PORT"
```

Key rules:
- **Always source `/opt/lap/common.sh`** — it handles vault injection, git clone, `REPO_DIR`, `PORT`
- **Always validate BOOT_MODEL** — harness must exit 1 if gateway returns no models
- **Never hardcode credentials** — read from env vars injected by common.sh/vault
- **Use `exec`** for the final command so the process is PID 1

### `harnesses/<name>/gen-mcp-config.mjs`

Outputs a JSON object of MCP server configs. Copy from `harnesses/opencode/gen-mcp-config.mjs` and adapt for the runtime's MCP format (stdio vs remote, field names, etc.).

### `harnesses/<name>/package.json`

```json
{
  "name": "<name>-sandbox-mcp",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "description": "stdio MCP server for <Name> harness (sandbox tools)",
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.12.0",
    "e2b": "^1.13.2"
  }
}
```

---

## Step 4: Scaffold `templates/<name>/` in litellm-agent-platform-2

Clone `litellm-agent-platform-2` if not already present, then create `templates/<name>/`:

```
templates/<name>/
  Dockerfile          # builds the harness image
  package.json        # if Node-based
  src/                # server source (if the harness needs a wrapper API like opencode does)
  scripts/
    smoke.sh          # end-to-end smoke test
  docs/
    eks-deployment.md # EKS + optional sandbox deployment guide
    eks-deploy-prompt.md  # agent prompt for hands-free deployment
  README.md
  render.yaml         # Render.com deploy config
```

### `Dockerfile`

Install the runtime, copy source, set env defaults:

```dockerfile
FROM node:20   # or python:3.12, golang:1.22, etc. — match runtime language

# Install runtime CLI
RUN <install command>

WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev   # if applicable
COPY src ./src

RUN mkdir -p /data /tmp/<name>-workspace
ENV PORT=8080 WORKDIR=/tmp/<name>-workspace DB_PATH=/data/agents.db

EXPOSE 8080
CMD ["<start command>"]
```

### `README.md`

Follow the opencode template README pattern:
- Diagram (GKE/EKS box with Agent Control Plane → Agent Server → Sandbox)
- Quickstart (Docker + local)
- LAP SDK snippet
- Environment variables table
- Deploy on EKS section (collapsed `<details>`)

### `scripts/smoke.sh`

End-to-end test: health → create agent → create session → send message → assert reply. Use `claude-sonnet-4-6` as the default model.

### `docs/eks-deployment.md`

Deployment guide with agent prompt at the top (link to `eks-deploy-prompt.md`), full manual steps in a `<details>` dropdown. Include harness-specific gotchas discovered during research.

---

## Step 5: Wire into the Platform

### Update `harnesses/plugin-registry.mjs`

Check if it exists; if so, add the new harness to the registry:

```js
import { createPlugin as create<Name>Plugin } from "./<name>/plugin.mjs";
// ... register in the map
```

If no registry exists, check `harnesses/harness-sdk.mjs` and `src/` for where harness names are declared, and add `<name>` to those lists.

### Update `harnesses/README.md` (if present)

Add a row for the new harness.

### Check `ui/` for harness selector

If the UI has a harness dropdown (search for `opencode` in `ui/src/`), add `<name>` there too.

---

## Step 6: Verify Structure

```bash
# Confirm harness entrypoint is executable
chmod +x harnesses/<name>/entrypoint.sh

# Lint the entrypoint (bash -n = syntax check only)
bash -n harnesses/<name>/entrypoint.sh

# Confirm template Dockerfile builds
docker build --platform linux/amd64 -t <name>-harness-test templates/<name>/
# (only if Docker is available and runtime image isn't huge)

# Run smoke test if server is running
BASE=http://localhost:8080 MODEL=claude-sonnet-4-6 templates/<name>/scripts/smoke.sh
```

---

## Step 7: File PR

1. Commit harness files to a branch: `feat/harness-<name>`
2. Commit template files to `litellm-agent-platform-2` on a branch: `feat/template-<name>`
3. File both PRs with:
   - What the harness does
   - How it wires to the LiteLLM gateway
   - Known limitations or TODOs (e.g. "MCP not yet supported upstream")
   - Link to any relevant upstream docs/issues

---

## Reference: opencode integration

| File | Purpose |
|------|---------|
| `harnesses/opencode/entrypoint.sh` | Complete working entrypoint — the canonical example |
| `harnesses/opencode/gen-mcp-config.mjs` | MCP config generator — copy and adapt |
| `templates/opencode/Dockerfile` | Image build pattern |
| `templates/opencode/src/` | Anthropic Managed Agents API wrapper server |
| `templates/opencode/docs/eks-deployment.md` | Deployment guide format |
| `templates/opencode/README.md` | README format with diagram |

When in doubt, read the opencode files and adapt the pattern. The key invariants shared by every harness:
1. Source `/opt/lap/common.sh`
2. Validate BOOT_MODEL or exit 1
3. Wire MCP via gen-mcp-config
4. Write native config with injected gateway URL/key/models
5. `exec` the runtime as PID 1
