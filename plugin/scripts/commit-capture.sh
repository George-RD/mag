#!/bin/sh
# MAG commit-capture — auto-capture jj/git commit messages as Decision memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: event JSON via stdin (tool_input.command, tool_response.stdout)
set -eu

MAG_DATA_ROOT="${MAG_DATA_ROOT:-$HOME/.mag}"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time*1000' 2>/dev/null || printf '%s000' "$(date +%s)"
}

# PostToolUse hooks receive the event payload via stdin (JSON), not env vars.
# CLAUDE_TOOL_INPUT / CLAUDE_TOOL_OUTPUT are legacy env vars that may be empty.
# Always read stdin for reliable cross-version behavior.
STDIN_PAYLOAD="$(cat 2>/dev/null)" || STDIN_PAYLOAD=""

# Extract command from stdin payload (preferred) or legacy env var.
if [ -n "$STDIN_PAYLOAD" ] && command -v jq >/dev/null 2>&1; then
  INPUT="$(printf '%s' "$STDIN_PAYLOAD" | jq -r '.tool_input.command // empty' 2>/dev/null || true)"
else
  INPUT="${CLAUDE_TOOL_INPUT:-}"
  if command -v jq >/dev/null 2>&1; then
    INPUT="$(printf '%s' "$INPUT" | jq -r '.command // empty' 2>/dev/null || true)"
  fi
fi

case "$INPUT" in
  *"jj commit"*|*"jj describe"*|*"git commit"*) ;;
  *) exit 0 ;;
esac

START_TS=$(now_ms)

COMMAND="$INPUT"

# Detect VCS tool
VCS_TOOL="git"
case "$COMMAND" in
  *"jj commit"*|*"jj describe"*) VCS_TOOL="jj" ;;
esac

# Extract commit message from -m flag (quoted, then unquoted fallback)
MSG="$(printf '%s' "$COMMAND" | sed -nE "s/.*-m[[:space:]]+['\"]([^'\"]*)['\"].*/\1/p" | head -1 || true)"
if [ -z "$MSG" ]; then
  MSG="$(printf '%s' "$COMMAND" | sed -nE 's/.*-m[[:space:]]+([^[:space:];|&]+).*/\1/p' | head -1 || true)"
fi

# Fallback: parse jj output for "Working copy now at: <hash> <message>"
if [ -z "$MSG" ]; then
  # Extract stdout from stdin payload (preferred) or legacy env var
  if [ -n "$STDIN_PAYLOAD" ] && command -v jq >/dev/null 2>&1; then
    OUTPUT="$(printf '%s' "$STDIN_PAYLOAD" | jq -r '.tool_response.stdout // .tool_response.output // empty' 2>/dev/null || true)"
  else
    OUTPUT="$(printf '%s' "${CLAUDE_TOOL_OUTPUT:-}" | jq -r '.output // empty' 2>/dev/null || true)"
  fi
  MSG="$(printf '%s' "$OUTPUT" | sed -n 's/Working copy now at: [a-z0-9]* //p' | head -1 | head -c 200 || true)"
fi

[ -n "$MSG" ] || exit 0

PROJECT="$(basename "$PWD")"
# session_id comes from stdin payload (not env var)
SESSION_ID=""
if [ -n "$STDIN_PAYLOAD" ] && command -v jq >/dev/null 2>&1; then
  SESSION_ID="$(printf '%s' "$STDIN_PAYLOAD" | jq -r '.session_id // empty' 2>/dev/null || true)"
fi
SESSION_ID="${SESSION_ID:-${CLAUDE_SESSION_ID:-}}"

mkdir -p "$MAG_DATA_ROOT"

# Invoke mag and capture exit code
MAG_EXIT=0
mag process "Commit: $MSG" \
  --event-type git_commit \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 2>/dev/null || MAG_EXIT=$?

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
  # Reflect actual store result: stored:true only when mag exited successfully
  if [ "$MAG_EXIT" -eq 0 ]; then
    MEM_BLOCK="$(jq -n --arg content "Commit: $MSG" --arg proj "$PROJECT" \
      '{stored:true,content:$content,project:$proj,event_type:"git_commit"}' 2>/dev/null || printf 'null')"
  else
    MEM_BLOCK="$(jq -n --arg proj "$PROJECT" --arg exit_code "$MAG_EXIT" \
      '{stored:false,project:$proj,event_type:"git_commit",error:("mag exited " + $exit_code)}' 2>/dev/null || printf 'null')"
  fi
  jq -nc \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg session_id "$SESSION_ID" \
    --arg proj "$PROJECT" \
    --arg dur "$DURATION_MS" \
    --arg status "$HOOK_STATUS" \
    --argjson err "$HOOK_ERROR" \
    --argjson mem "$MEM_BLOCK" \
    --arg commit_msg "$MSG" \
    --arg vcs "$VCS_TOOL" \
    '{v:0,ts:$ts,event:"hook.commit_capture",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"commit-capture",duration_ms:($dur|tonumber),status:$status,error:$err},memory:$mem,context:{commit_message:$commit_msg,vcs_tool:$vcs}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Degraded output: jq unavailable. Some fields omitted. Install jq for full telemetry.
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | sed 's/\\/\\\\/g; s/"/\\"/g')
  if [ "$HOOK_STATUS" = "error" ]; then
    printf '{"v":0,"ts":"%s","event":"hook.commit_capture","session_id":null,"project":"%s","hook":{"name":"commit-capture","duration_ms":%s,"status":"%s","error":"%s"}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" "$SAFE_ERROR" \
      >> "$LOG" 2>/dev/null || true
  else
    printf '{"v":0,"ts":"%s","event":"hook.commit_capture","session_id":null,"project":"%s","hook":{"name":"commit-capture","duration_ms":%s,"status":"%s","error":null}}\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" "$HOOK_STATUS" \
      >> "$LOG" 2>/dev/null || true
  fi
fi
