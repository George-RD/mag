#!/bin/sh
# MAG dev subagent end — store subagent summary, emit JSONL telemetry
# SubagentStop hook: reads stdin JSON from Claude Code
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
START_TS=$(now_ms)

# Read stdin JSON (SubagentStop provides session_id, agent_id, agent_type, etc.)
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  AGENT_ID=$(printf '%s' "$INPUT" | jq -r '.agent_id // empty' 2>/dev/null) || AGENT_ID=""
  AGENT_TYPE=$(printf '%s' "$INPUT" | jq -r '.agent_type // empty' 2>/dev/null) || AGENT_TYPE=""
  LAST_MSG=$(printf '%s' "$INPUT" | jq -r '.last_assistant_message // empty' 2>/dev/null) || LAST_MSG=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
else
  SESSION_ID=""
  AGENT_ID=""
  AGENT_TYPE=""
  LAST_MSG=""
  CWD=""
fi

SESSION_ID="${SESSION_ID:-}"
AGENT_ID="${AGENT_ID:-unknown}"
AGENT_TYPE="${AGENT_TYPE:-unknown}"
LAST_MSG="${LAST_MSG:-}"
CWD="${CWD:-$PWD}"
PROJECT="$(basename "$CWD")"

mkdir -p "$MAG_DATA_ROOT"

# Truncate last_assistant_message to 200 chars
MSG_PREVIEW=""
if [ -n "$LAST_MSG" ]; then
  MSG_PREVIEW="$(printf '%.200s' "$LAST_MSG")"
  SUMMARY="Subagent ended. Agent: $AGENT_ID ($AGENT_TYPE). Project: $PROJECT. Last message: $MSG_PREVIEW"
else
  SUMMARY="Subagent ended. Agent: $AGENT_ID ($AGENT_TYPE). Project: $PROJECT."
fi

# Store with lower importance than main session (D4: 0.3)
MAG_EXIT=0
mag process "$SUMMARY" \
  --event-type subagent_end \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.3 2>/dev/null || MAG_EXIT=$?

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
  if [ -n "$SESSION_ID" ]; then SID_JSON="\"$SESSION_ID\""; else SID_JSON="null"; fi
  jq -nc \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson session_id "$SID_JSON" \
    --arg proj "$PROJECT" \
    --arg agent_id "$AGENT_ID" \
    --arg agent_type "$AGENT_TYPE" \
    --arg dur "$DURATION_MS" \
    --arg status "$HOOK_STATUS" \
    --argjson err "$HOOK_ERROR" \
    --arg msg_preview "${MSG_PREVIEW:-}" \
    '{v:0,ts:$ts,event:"hook.subagent_end",session_id:$session_id,project:$proj,agent:{id:$agent_id,type:$agent_type,tool:"claude_code"},hook:{name:"subagent-end",duration_ms:($dur|tonumber),status:$status,error:$err},memory:null,context:{last_assistant_message:$msg_preview}}' \
    >> "$LOG" 2>/dev/null || true
else
  SAFE_AGENT_ID=$(printf '%s' "$AGENT_ID" | tr -d '"\\')
  SAFE_AGENT_TYPE=$(printf '%s' "$AGENT_TYPE" | tr -d '"\\')
  SAFE_MSG_PREVIEW=$(printf '%s' "${MSG_PREVIEW:-}" | tr -d '"\\' | head -c 200)
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | tr -d '"\\')
  if [ "$HOOK_STATUS" = "error" ]; then
    printf '{"v":0,"ts":"%s","event":"hook.subagent_end","session_id":null,"project":"%s","agent":{"id":"%s","type":"%s","tool":"claude_code"},"hook":{"name":"subagent-end","duration_ms":%s,"status":"%s","error":"%s"},"memory":null,"context":{"last_assistant_message":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SAFE_AGENT_ID" "$SAFE_AGENT_TYPE" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_ERROR" "$SAFE_MSG_PREVIEW" \
      >> "$LOG" 2>/dev/null || true
  else
    printf '{"v":0,"ts":"%s","event":"hook.subagent_end","session_id":null,"project":"%s","agent":{"id":"%s","type":"%s","tool":"claude_code"},"hook":{"name":"subagent-end","duration_ms":%s,"status":"%s","error":null},"memory":null,"context":{"last_assistant_message":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SAFE_AGENT_ID" "$SAFE_AGENT_TYPE" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_MSG_PREVIEW" \
      >> "$LOG" 2>/dev/null || true
  fi
fi
