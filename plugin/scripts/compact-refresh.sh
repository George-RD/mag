#!/bin/sh
# MAG post-compact — re-inject memories + restore pre-compact state, emit JSONL
set -eu

MAG_DATA_ROOT="${MAG_DATA_ROOT:-$HOME/.mag}"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
STATE_DIR="$MAG_DATA_ROOT/state"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time*1000' 2>/dev/null || printf '%s000' "$(date +%s)"
}
START_TS=$(now_ms)
mkdir -p "$MAG_DATA_ROOT" "$STATE_DIR"

# Read stdin JSON for session context
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
  COMPACT_SUMMARY=$(printf '%s' "$INPUT" | jq -r '.compact_summary // empty' 2>/dev/null) || COMPACT_SUMMARY=""
else
  SESSION_ID="${CLAUDE_SESSION_ID:-}"
  CWD="$PWD"
  COMPACT_SUMMARY=""
fi

SESSION_ID="${SESSION_ID:-${CLAUDE_SESSION_ID:-}}"
CWD="${CWD:-$PWD}"
PROJECT="$(basename "$CWD")"

# Re-inject top memories (full budget — context is smallest after compaction)
# Capture output: PostCompact hooks return additionalContext to Claude via stdout
WELCOME_OUTPUT=""
WELCOME_OUTPUT=$(mag welcome --project "$PROJECT" --session-id "$SESSION_ID" --budget-tokens 2000 2>/dev/null) || true

# Store compact summary as a memory if available
if [ -n "$COMPACT_SUMMARY" ]; then
  mag process "$COMPACT_SUMMARY" \
    --event-type session_end \
    --project "$PROJECT" \
    --session-id "$SESSION_ID" \
    --importance 0.3 2>/dev/null || true
fi

# Restore pre-compact snapshot if one was saved
SNAPSHOT="$STATE_DIR/pre-compact-$SESSION_ID.json"
RESTORE=""
if [ -f "$SNAPSHOT" ] && command -v jq >/dev/null 2>&1; then
  WD=$(jq -r '.working_directory // empty' "$SNAPSHOT" 2>/dev/null) || WD=""
  RF=$(jq -r '.recent_file // empty' "$SNAPSHOT" 2>/dev/null) || RF=""
  VCS=$(jq -r '.vcs_state // empty' "$SNAPSHOT" 2>/dev/null) || VCS=""

  [ -n "$WD" ] && RESTORE="${RESTORE}Working directory: $WD
"
  [ -n "$RF" ] && RESTORE="${RESTORE}Recent file: $RF
"
  [ -n "$VCS" ] && RESTORE="${RESTORE}VCS state: $VCS
"

  rm -f "$SNAPSHOT"
fi

# Emit additionalContext to stdout (consumed by Claude Code PostCompact contract)
# Always emit — even without jq — so compaction recovery is never a silent no-op.
CONTEXT_PARTS=""
[ -n "$WELCOME_OUTPUT" ] && CONTEXT_PARTS="$WELCOME_OUTPUT"
if [ -n "$RESTORE" ]; then
  CONTEXT_PARTS="${CONTEXT_PARTS:+${CONTEXT_PARTS}
}<MAG_RESTORE>Pre-compact state recovered:
${RESTORE}</MAG_RESTORE>"
fi
if [ -n "$CONTEXT_PARTS" ]; then
  if command -v jq >/dev/null 2>&1; then
    jq -n --arg r "$CONTEXT_PARTS" '{"additionalContext": $r}' 2>/dev/null || true
  else
    # Fallback: emit a safe JSON object without jq (strip special chars that break JSON)
    SAFE_CONTEXT=$(printf '%s' "$CONTEXT_PARTS" | tr -d '\000-\037' | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '{"additionalContext":"%s"}\n' "$SAFE_CONTEXT"
  fi
fi

END_TS=$(now_ms)
DURATION_MS=$(( END_TS - START_TS ))

# Emit JSONL — truncate compact_summary preview to 200 chars
if command -v jq >/dev/null 2>&1; then
  SUMMARY_PREVIEW="$(printf '%.200s' "$COMPACT_SUMMARY")"
  jq -nc \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg session_id "$SESSION_ID" \
    --arg proj "$PROJECT" \
    --arg dur "$DURATION_MS" \
    --arg summary_preview "$SUMMARY_PREVIEW" \
    '{v:0,ts:$ts,event:"hook.post_compact",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"compact-refresh",duration_ms:($dur|tonumber),status:"ok",error:null},memory:null,context:{compact_summary:$summary_preview}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Degraded output: jq unavailable. Some fields omitted. Install jq for full telemetry.
  printf '{"v":0,"ts":"%s","event":"hook.post_compact","session_id":null,"project":"%s","hook":{"name":"compact-refresh","duration_ms":%s,"status":"ok","error":null}}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" \
    >> "$LOG" 2>/dev/null || true
fi

exit 0
