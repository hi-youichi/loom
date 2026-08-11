#!/usr/bin/env bash
# Linux/macOS equivalent of check-protocol.ps1.
set -euo pipefail

PORT="${LOOM_PROTOCOL_PORT:-18080}"
BASE_URL="${LOOM_PROTOCOL_BASE_URL:-http://127.0.0.1:${PORT}}"
AUTHORIZATION="${LOOM_PROTOCOL_AUTHORIZATION:-Basic dXNlcjp0ZXN0}"
NO_BOOT="${LOOM_PROTOCOL_NO_BOOT:-0}"
SERVER_PID=""
SESSION_ID=""
STDOUT_LOG="$(mktemp)"
STDERR_LOG="$(mktemp)"

cleanup() {
  if [[ -n "$SESSION_ID" ]]; then
    curl -sS -X DELETE -H "Authorization: ${AUTHORIZATION}" "${BASE_URL}/session/${SESSION_ID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$STDOUT_LOG" "$STDERR_LOG"
}
trap cleanup EXIT

request() {
  local method="$1" path="$2" body="${3-}" expected="${4:-200,204}"
  local args=(-sS -o /tmp/loom-protocol-body.$$ -w '%{http_code}' -X "$method" -H "Authorization: ${AUTHORIZATION}")
  if [[ -n "$body" ]]; then
    args+=(-H 'Content-Type: application/json' --data "$body")
  fi
  local status
  status="$(curl "${args[@]}" "${BASE_URL}${path}")"
  if [[ ",${expected}," != *",${status},"* ]]; then
    printf '%s %s returned HTTP %s: %s\n' "$method" "$path" "$status" "$(cat /tmp/loom-protocol-body.$$)" >&2
    return 1
  fi
  cat /tmp/loom-protocol-body.$$
  rm -f /tmp/loom-protocol-body.$$
}

if [[ "$NO_BOOT" != "1" ]]; then
  cargo run -p loom-server -- serve --host 127.0.0.1 --port "$PORT" >"$STDOUT_LOG" 2>"$STDERR_LOG" &
  SERVER_PID=$!
fi

for _ in $(seq 1 360); do
  if request GET /api/health '' 200 >/dev/null 2>&1; then break; fi
  if [[ -n "$SERVER_PID" ]] && ! kill -0 "$SERVER_PID" 2>/dev/null; then
    cat "$STDERR_LOG" >&2
    exit 1
  fi
  sleep 0.5
done
request GET /api/health '' 200 >/dev/null

for path in \
  /config /config/providers /provider /agent /path /project/current /command \
  /mcp /mcp/status /lsp /formatter /session/status /provider/auth /vcs \
  /experimental/resource/list /experimental/capabilities \
  /api/health /api/location /api/path /api/app/agent /api/app/model \
  /api/app/provider /global/health; do
  request GET "$path" '' 200 >/dev/null
done

for spec in '/global/event:v1' '/api/event:v2'; do
  path="${spec%%:*}"
  kind="${spec##*:}"
  set +e
  frame="$(curl -sN --max-time 3 -H "Authorization: ${AUTHORIZATION}" "${BASE_URL}${path}")"
  rc=$?
  set -e
  [[ $rc -eq 0 || $rc -eq 28 ]] || exit "$rc"
  data="$(printf '%s\n' "$frame" | sed -n 's/^data: *//p' | sed -n '1p')"
  [[ -n "$data" ]] || { echo "SSE ${path} emitted no data frame" >&2; exit 1; }
  if [[ "$kind" == v1 ]]; then
    printf '%s' "$data" | grep -q '"directory"'
    printf '%s' "$data" | grep -q '"payload"'
  else
    printf '%s' "$data" | grep -q '"id"'
    printf '%s' "$data" | grep -q '"payload"'
  fi
done

created="$(request POST /session '{"title":"protocol gate"}' 200)"
SESSION_ID="$(printf '%s' "$created" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')"
[[ "$SESSION_ID" == sess_* ]] || { echo 'invalid session id' >&2; exit 1; }
request PATCH "/session/${SESSION_ID}" '{"title":"protocol gate updated"}' 200 >/dev/null
request GET "/session/${SESSION_ID}" '' 200 >/dev/null
request GET "/session/${SESSION_ID}/message" '' 200 >/dev/null
request POST "/session/${SESSION_ID}/shell" '{"command":"echo loom-shell"}' 200 | grep -q 'loom-shell'
request POST "/session/${SESSION_ID}/abort" '{}' 200 >/dev/null
request GET "/api/session/${SESSION_ID}/event" '' 200 >/dev/null

request GET /permission '' 200 >/dev/null
request GET /question '' 200 >/dev/null
request PATCH /mcp '{}' 200 >/dev/null
request POST /mcp/protocol/connect '{}' 200 >/dev/null
request GET /pty '' 200 >/dev/null
request GET /file/status '' 200 >/dev/null
request GET '/find?pattern=src' '' 200 >/dev/null
request GET /experimental/resource/protocol '' 200 >/dev/null
request POST /provider/auth '{"providerID":"protocol"}' 200 >/dev/null
request POST /api/instance '{}' 200 >/dev/null
request PUT /api/location/workspace '{}' 200 >/dev/null
request POST /api/mcp '{}' 200 >/dev/null
request GET /api/experimental/app '' 200 >/dev/null
request GET /global/version '' 200 >/dev/null
request DELETE "/session/${SESSION_ID}" '' 204 >/dev/null
SESSION_ID=""

echo 'Protocol gate passed: bootstrap, SSE v1/v2, session CRUD, auth pass-through, and P2 routes.'
