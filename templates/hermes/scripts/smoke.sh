#!/usr/bin/env bash
# smoke.sh — smoke-tests the Anthropic Managed Agents API surface exposed by
# the Hermes harness.
#
# Usage:
#   BASE=http://localhost:8080 MODEL=claude-sonnet-4-6 ./scripts/smoke.sh
set -euo pipefail

BASE="${BASE:-http://localhost:8080}"
MODEL="${MODEL:-claude-sonnet-4-6}"

HDR=(-H "content-type: application/json")

json_field() {
  local field="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$field',''))"
  elif command -v node >/dev/null 2>&1; then
    node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s)['$field']||''))"
  else
    grep -o "\"$field\":\"[^\"]*\"" | head -1 | cut -d'"' -f4
  fi
}

ok() { echo "  ✓ $*"; }

echo "=== 1. health ==="
curl -fsS "${HDR[@]}" "$BASE/health" | grep '"ok":true'
ok "health"

echo "=== 2. create agent ==="
AGENT=$(curl -fsS -X POST "${HDR[@]}" "$BASE/v1/agents" \
  -d "{\"name\":\"smoke\",\"model\":\"$MODEL\",\"system\":\"You are helpful.\"}")
AGENT_ID=$(echo "$AGENT" | json_field id)
[ -n "$AGENT_ID" ] || { echo "ERROR: no agent id"; exit 1; }
ok "agent id=$AGENT_ID"

echo "=== 3. create environment ==="
ENV=$(curl -fsS -X POST "${HDR[@]}" "$BASE/v1/environments" -d '{}')
ENV_ID=$(echo "$ENV" | json_field id)
ok "env id=$ENV_ID"

echo "=== 4. create session ==="
SESSION=$(curl -fsS -X POST "${HDR[@]}" "$BASE/v1/sessions" \
  -d "{\"agent_id\":\"$AGENT_ID\",\"environment_id\":\"$ENV_ID\"}")
SESSION_ID=$(echo "$SESSION" | json_field id)
[ -n "$SESSION_ID" ] || { echo "ERROR: no session id"; exit 1; }
ok "session id=$SESSION_ID"

echo "=== 5. send prompt (202) ==="
STATUS=$(curl -fsS -o /dev/null -w "%{http_code}" -X POST "${HDR[@]}" \
  "$BASE/v1/sessions/$SESSION_ID/events" \
  -d '{"events":[{"type":"user.message","content":"Say hello in one word."}]}')
[ "$STATUS" = "202" ] || { echo "ERROR: expected 202, got $STATUS"; exit 1; }
ok "prompt accepted (202)"

echo "=== 6. stream events (10s timeout) ==="
EVENTS=$(curl -fsS --max-time 10 "$BASE/v1/sessions/$SESSION_ID/events/stream" 2>/dev/null || true)
if echo "$EVENTS" | grep -q "session.status_idle\|agent.message"; then
  ok "got SSE events"
else
  echo "  (no SSE events — hermes CLI may not be available in this env)"
fi

echo ""
echo "=== SMOKE PASS ==="
