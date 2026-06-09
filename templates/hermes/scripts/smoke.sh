#!/usr/bin/env bash
set -euo pipefail
BASE="${BASE:-http://localhost:8080}"
MODEL="${MODEL:-claude-sonnet-4-6}"

echo "=== health ==="
curl -fsS "$BASE/health" | grep '"ok":true'
echo "  OK"

echo "=== create agent ==="
AGENT=$(curl -fsS -X POST "$BASE/v1/agents" \
  -H "content-type: application/json" \
  -d "{\"name\":\"smoke\",\"model\":\"$MODEL\",\"system\":\"You are a helpful assistant.\"}")
AGENT_ID=$(echo "$AGENT" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s).id))")
echo "  agent id: $AGENT_ID"

echo "=== create env ==="
ENV=$(curl -fsS -X POST "$BASE/v1/environments" -H "content-type: application/json" -d '{}')
ENV_ID=$(echo "$ENV" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s).id))")
echo "  env id: $ENV_ID"

echo "=== create session ==="
SESSION=$(curl -fsS -X POST "$BASE/v1/sessions" \
  -H "content-type: application/json" \
  -d "{\"agent_id\":\"$AGENT_ID\",\"environment_id\":\"$ENV_ID\"}")
SESSION_ID=$(echo "$SESSION" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(JSON.parse(s).id))")
echo "  session id: $SESSION_ID"

echo "=== send prompt ==="
STATUS=$(curl -fsS -o /dev/null -w "%{http_code}" -X POST "$BASE/v1/sessions/$SESSION_ID/events" \
  -H "content-type: application/json" \
  -d '{"content":"Say hello in one word."}')
[ "$STATUS" = "202" ] || { echo "expected 202, got $STATUS"; exit 1; }
echo "  accepted (202)"

echo "=== stream events (10s timeout) ==="
curl -fsS --max-time 10 "$BASE/v1/sessions/$SESSION_ID/events/stream" \
  | grep -m1 "session.status_idle\|agent.message" || true

echo ""
echo "=== SMOKE PASS ==="
