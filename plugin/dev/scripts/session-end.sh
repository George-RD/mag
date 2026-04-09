#!/bin/sh
# MAG dev session end — store lightweight session summary, emit JSONL telemetry
# Stop hook: reads stdin JSON from Claude Code
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time*1000' 2>/dev/null || printf '%s000' "$(date +%s)"
}
START_TS=$(now_ms)

# Read stdin JSON (Stop hook provides session_id, last_assistant_message, transcript_path, etc.)
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
  LAST_MSG=$(printf '%s' "$INPUT" | jq -r '.last_assistant_message // empty' 2>/dev/null) || LAST_MSG=""
  TRANSCRIPT_PATH=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null) || TRANSCRIPT_PATH=""
  HOOK_EVENT=$(printf '%s' "$INPUT" | jq -r '.hook_event_name // empty' 2>/dev/null) || HOOK_EVENT=""
else
  SESSION_ID=""
  CWD=""
  LAST_MSG=""
  TRANSCRIPT_PATH=""
  HOOK_EVENT=""
fi

SESSION_ID="${SESSION_ID:-}"
CWD="${CWD:-$PWD}"
LAST_MSG="${LAST_MSG:-}"
TRANSCRIPT_PATH="${TRANSCRIPT_PATH:-}"
PROJECT="$(basename "$CWD")"

# SubagentStop events are handled by subagent-end.sh — skip here
if [ "$HOOK_EVENT" = "SubagentStop" ]; then
  exit 0
fi

mkdir -p "$MAG_DATA_ROOT"

# Truncate last_assistant_message to 200 chars for summary
MSG_PREVIEW=""
if [ -n "$LAST_MSG" ]; then
  MSG_PREVIEW="$(printf '%.200s' "$LAST_MSG")"
  SUMMARY="Session ended. Project: $PROJECT. Last assistant message: $MSG_PREVIEW"
else
  SUMMARY="Session ended. Project: $PROJECT."
fi

# Invoke mag and capture exit code
MAG_EXIT=0
mag process "$SUMMARY" \
  --event-type session_end \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.4 2>/dev/null || MAG_EXIT=$?

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
    --arg msg_preview "${MSG_PREVIEW:-}" \
    --arg transcript "${TRANSCRIPT_PATH:-}" \
    '{v:0,ts:$ts,event:"hook.session_end",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"session-end",duration_ms:($dur|tonumber),status:$status,error:$err},memory:null,context:{last_assistant_message:$msg_preview,transcript_path:$transcript}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Degraded output: jq unavailable. Some fields omitted. Install jq for full telemetry.
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | sed 's/\\/\\\\/g; s/"/\\"/g')
  if [ "$HOOK_STATUS" = "error" ]; then
    printf '{"v":0,"ts":"%s","event":"hook.session_end","session_id":null,"project":"%s","hook":{"name":"session-end","duration_ms":%s,"status":"%s","error":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_ERROR" \
      >> "$LOG" 2>/dev/null || true
  else
    printf '{"v":0,"ts":"%s","event":"hook.session_end","session_id":null,"project":"%s","hook":{"name":"session-end","duration_ms":%s,"status":"%s","error":null}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" \
      >> "$LOG" 2>/dev/null || true
  fi
fi
