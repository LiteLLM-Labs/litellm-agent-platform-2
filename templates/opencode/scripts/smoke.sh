#!/usr/bin/env bash
#
# smoke.sh — smoke-tests the opencode-compatible HTTP CONTRACT against a running
# opencode-agent-server. Exercises health, agent CRUD, session create, async
# prompt + SSE stream, and session delete.
#
# Usage:
#   BASE=http://localhost:8080 MODEL=anthropic/claude-sonnet-4-5 ./scripts/smoke.sh
#
# Make it executable first:  chmod +x scripts/smoke.sh
#
# NOTE: the prompt round-trip (steps 5-7) only produces real assistant output if
# the SERVER's environment has a model provider key (e.g. ANTHROPIC_API_KEY).
# Without it, agent/session creation still succeed; the SSE stream just won't
# carry assistant message parts.

set -euo pipefail

BASE="${BASE:-http://localhost:8080}"
MODEL="${MODEL:-anthropic/claude-sonnet-4-5}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Extract a top-level string field from a JSON document on stdin.
# Prefers python3; falls back to node, then jq.
json_field() {
  local field="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 -c "import sys,json;print(json.load(sys.stdin).get('$field',''))"
  elif command -v node >/dev/null 2>&1; then
    node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{try{process.stdout.write(String((JSON.parse(s)||{})['$field']??''))}catch(e){process.exit(0)}})"
  elif command -v jq >/dev/null 2>&1; then
    jq -r ".$field // empty"
  else
    echo "ERROR: need python3, node, or jq to parse JSON" >&2
    return 1
  fi
}

label() {
  echo ""
  echo "==> $*"
}

# ---------------------------------------------------------------------------
# 1. Health
# ---------------------------------------------------------------------------

label "GET /health"
HEALTH=$(curl -s "$BASE/health")
echo "$HEALTH"
OK=$(printf '%s' "$HEALTH" | json_field ok)
if [ "$OK" != "True" ] && [ "$OK" != "true" ]; then
  echo "FAIL: /health did not report ok" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 2. Create an agent
# ---------------------------------------------------------------------------

label "POST /agents (Smoke Test)"
AGENT=$(curl -s "$BASE/agents" \
  -H 'content-type: application/json' \
  -d "{
    \"name\": \"Smoke Test\",
    \"system\": \"You are a terse assistant.\",
    \"model\": \"$MODEL\",
    \"permissions\": { \"bash\": \"deny\", \"edit\": \"deny\" }
  }")
echo "$AGENT"
AID=$(printf '%s' "$AGENT" | json_field id)
if [ -z "$AID" ]; then
  echo "FAIL: no agent id returned from POST /agents" >&2
  exit 1
fi
echo "agent id: $AID"

# ---------------------------------------------------------------------------
# 3. Fetch the agent back
# ---------------------------------------------------------------------------

label "GET /agents/$AID"
curl -s "$BASE/agents/$AID"
echo ""

# ---------------------------------------------------------------------------
# 4. Create a session bound to the agent
# ---------------------------------------------------------------------------

label "POST /session (agent=$AID)"
SESSION=$(curl -s "$BASE/session" \
  -H 'content-type: application/json' \
  -d "{\"agent\":\"$AID\"}")
echo "$SESSION"
SID=$(printf '%s' "$SESSION" | json_field id)
if [ -z "$SID" ]; then
  echo "FAIL: no session id returned from POST /session" >&2
  exit 1
fi
echo "session id: $SID"

# ---------------------------------------------------------------------------
# 5. Subscribe to /event in the background
# ---------------------------------------------------------------------------

label "GET /event (background, ~8s)"
EVENTS_FILE=$(mktemp)
curl -sN "$BASE/event" | tee "$EVENTS_FILE" >/dev/null &
EV_PID=$!
# Give the SSE connection a moment to establish before prompting.
sleep 1

# ---------------------------------------------------------------------------
# 6. Prompt asynchronously (requires a model key in the SERVER env to respond)
# ---------------------------------------------------------------------------

label "POST /session/$SID/prompt_async"
PROMPT_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
  -X POST "$BASE/session/$SID/prompt_async" \
  -H 'content-type: application/json' \
  -d '{"parts":[{"type":"text","text":"Say hello in 3 words."}]}')
echo "HTTP $PROMPT_CODE (expect 204)"

# ---------------------------------------------------------------------------
# 7. Wait for events, then stop the SSE stream and print what arrived
# ---------------------------------------------------------------------------

label "waiting ~8s for events on session $SID"
sleep 8
kill "$EV_PID" 2>/dev/null || true
wait "$EV_PID" 2>/dev/null || true
echo "--- events received ---"
cat "$EVENTS_FILE"
echo "--- end events ---"
rm -f "$EVENTS_FILE"

# ---------------------------------------------------------------------------
# 8. Delete the session (GET /session/:id is not part of the contract)
# ---------------------------------------------------------------------------

label "DELETE /session/$SID"
DEL_CODE=$(curl -s -o /tmp/smoke_del_body -w '%{http_code}' -X DELETE "$BASE/session/$SID")
echo "HTTP $DEL_CODE"
cat /tmp/smoke_del_body 2>/dev/null || true
rm -f /tmp/smoke_del_body
echo ""

label "smoke test complete"
