#!/bin/sh
# MAG dev session start — recall project context, emit JSONL telemetry
# Outputs memory context for Claude Code injection
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time*1000' 2>/dev/null || printf '%s000' "$(date +%s)"
}
START_TS=$(now_ms)

# Read stdin JSON (SessionStart provides session_id, cwd, etc.)
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
else
  SESSION_ID=""
  CWD=""
fi

SESSION_ID="${SESSION_ID:-}"
CWD="${CWD:-$PWD}"
PROJECT="$(basename "$CWD")"

mkdir -p "$MAG_DATA_ROOT"

# Reap stale pre-compact snapshots (moved here from pre-compact.sh where timing is critical)
STATE_DIR="$MAG_DATA_ROOT/state"
if [ -d "$STATE_DIR" ]; then
  STALE_COUNT=$(ls "$STATE_DIR"/pre-compact-*.json 2>/dev/null | wc -l)
  if [ "$STALE_COUNT" -gt 10 ]; then
    find "$STATE_DIR" -name 'pre-compact-*.json' -mtime +1 -delete 2>/dev/null || true
  fi
fi

# Invoke mag and capture exit code
MAG_EXIT=0
mag welcome --project "$PROJECT" --session-id "$SESSION_ID" --budget-tokens 2000 2>/dev/null || MAG_EXIT=$?

END_TS=$(now_ms)
DURATION_MS=$(( END_TS - START_TS ))

HOOK_STATUS="ok"
HOOK_ERROR="null"
if [ "$MAG_EXIT" -ne 0 ]; then
  HOOK_STATUS="error"
  HOOK_ERROR="\"mag exited $MAG_EXIT\""
fi

# Emit JSONL
if command -v jq >/dev/null 2>&1; then
  jq -nc \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg session_id "$SESSION_ID" \
    --arg proj "$PROJECT" \
    --arg dur "$DURATION_MS" \
    --arg status "$HOOK_STATUS" \
    --argjson err "$HOOK_ERROR" \
    '{v:0,ts:$ts,event:"hook.session_start",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"session-start",duration_ms:($dur|tonumber),status:$status,error:$err},memory:null,context:{}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Degraded output: jq unavailable. Some fields omitted. Install jq for full telemetry.
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | sed 's/\\/\\\\/g; s/"/\\"/g')
  if [ "$HOOK_STATUS" = "error" ]; then
    printf '{"v":0,"ts":"%s","event":"hook.session_start","session_id":null,"project":"%s","hook":{"name":"session-start","duration_ms":%s,"status":"%s","error":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_ERROR" \
      >> "$LOG" 2>/dev/null || true
  else
    printf '{"v":0,"ts":"%s","event":"hook.session_start","session_id":null,"project":"%s","hook":{"name":"session-start","duration_ms":%s,"status":"%s","error":null}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" \
      >> "$LOG" 2>/dev/null || true
  fi
fi
