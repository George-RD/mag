#!/bin/sh
# MAG dev error-capture — auto-capture build/test failures as error_pattern memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (command JSON), $CLAUDE_TOOL_OUTPUT (output JSON)
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time*1000' 2>/dev/null || printf '%s000' "$(date +%s)"
}

# Fast-path: only process build/test commands
TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
case "$TOOL_INPUT" in
  *"cargo test"*|*"cargo build"*|*"cargo check"*|*"cargo clippy"*|*"npm test"*|*"npm run"*|*"prek run"*)
    ;; # fall through to failure detection
  *)
    exit 0
    ;;
esac

# Check output for failure signals
TOOL_OUTPUT="${CLAUDE_TOOL_OUTPUT:-}"
case "$TOOL_OUTPUT" in
  *"FAILED"*|*"error["*|*"error: "*|*"npm ERR!"*)
    ;; # fall through to error extraction
  *)
    exit 0
    ;;
esac

START_TS=$(now_ms)

# Extract first error line (Rust-style: starts with "error[E0XXX]:" or "error: ")
ERROR_LINE=$(printf '%s' "$TOOL_OUTPUT" | grep -m1 -E '^error(\[E[0-9]+\])?: ' 2>/dev/null || true)

# Fallback: npm-style errors
if [ -z "$ERROR_LINE" ]; then
  ERROR_LINE=$(printf '%s' "$TOOL_OUTPUT" | grep -m1 'npm ERR!' 2>/dev/null || true)
fi

# Fallback: first line containing FAILED
if [ -z "$ERROR_LINE" ]; then
  ERROR_LINE=$(printf '%s' "$TOOL_OUTPUT" | grep -m1 'FAILED' 2>/dev/null || true)
fi

# Nothing extracted — bail
[ -n "$ERROR_LINE" ] || exit 0

# Truncate to avoid oversized memory content
ERROR_LINE="$(printf '%.200s' "$ERROR_LINE")"

# Derive project and session (after fast-path so non-build commands skip this)
PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-}"

# Extract command preview for context
CMD_PREVIEW="$(printf '%s' "$TOOL_INPUT" | jq -r '.command // empty' 2>/dev/null | head -c 100 || true)"

mkdir -p "$MAG_DATA_ROOT"

# Invoke mag and capture exit code
MAG_EXIT=0
mag process "Build/test error in $PROJECT: $ERROR_LINE" \
  --event-type error_pattern \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 \
  2>/dev/null || MAG_EXIT=$?

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
    --arg error_line "$ERROR_LINE" \
    --arg cmd_preview "$CMD_PREVIEW" \
    '{v:0,ts:$ts,event:"hook.error_capture",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"error-capture",duration_ms:($dur|tonumber),status:$status,error:$err},memory:null,context:{error_line:$error_line,command_preview:$cmd_preview}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Degraded output: jq unavailable. Some fields omitted. Install jq for full telemetry.
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | sed 's/\\/\\\\/g; s/"/\\"/g')
  if [ "$HOOK_STATUS" = "error" ]; then
    printf '{"v":0,"ts":"%s","event":"hook.error_capture","session_id":null,"project":"%s","hook":{"name":"error-capture","duration_ms":%s,"status":"%s","error":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_ERROR" \
      >> "$LOG" 2>/dev/null || true
  else
    printf '{"v":0,"ts":"%s","event":"hook.error_capture","session_id":null,"project":"%s","hook":{"name":"error-capture","duration_ms":%s,"status":"%s","error":null}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" \
      >> "$LOG" 2>/dev/null || true
  fi
fi
