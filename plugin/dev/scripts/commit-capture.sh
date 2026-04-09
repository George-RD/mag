#!/bin/sh
# MAG dev commit-capture — auto-capture jj/git commit messages as Decision memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (JSON), $CLAUDE_TOOL_OUTPUT (JSON)
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
START_TS=$(now_ms)

# Fast-path rejection — plain string check before any process forks.
# CLAUDE_TOOL_INPUT is JSON like {"command":"jj commit -m ..."}, so a substring
# match on the raw string is safe and avoids the cost of jq for ~95% of calls.
INPUT="${CLAUDE_TOOL_INPUT:-}"
case "$INPUT" in
  *"jj commit"*|*"jj describe"*|*"git commit"*) ;;
  *) exit 0 ;;
esac

COMMAND="$(printf '%s' "$INPUT" | jq -r '.command // empty' 2>/dev/null || true)"

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
  OUTPUT="$(printf '%s' "${CLAUDE_TOOL_OUTPUT:-}" | jq -r '.output // empty' 2>/dev/null || true)"
  MSG="$(printf '%s' "$OUTPUT" | sed -n 's/Working copy now at: [a-z0-9]* //p' | head -1 | head -c 200 || true)"
fi

[ -n "$MSG" ] || exit 0

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-}"

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
  if [ -n "$SESSION_ID" ]; then SID_JSON="\"$SESSION_ID\""; else SID_JSON="null"; fi
  # Reflect actual store result: stored:true only when mag exited successfully
  if [ "$MAG_EXIT" -eq 0 ]; then
    MEM_BLOCK="$(jq -n --arg content "Commit: $MSG" --arg proj "$PROJECT" \
      '{stored:true,content:$content,project:$proj,event_type:"git_commit"}' 2>/dev/null || printf 'null')"
  else
    MEM_BLOCK="$(jq -n --arg proj "$PROJECT" --arg exit_code "$MAG_EXIT" \
      '{stored:false,project:$proj,event_type:"git_commit",error:("mag exited " + $exit_code)}' 2>/dev/null || printf 'null')"
  fi
  jq -n \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson session_id "$SID_JSON" \
    --arg proj "$PROJECT" \
    --arg dur "$DURATION_MS" \
    --arg status "$HOOK_STATUS" \
    --argjson err "$HOOK_ERROR" \
    --argjson mem "$MEM_BLOCK" \
    --arg commit_msg "$MSG" \
    --arg vcs "$VCS_TOOL" \
    '{v:0,ts:$ts,event:"hook.commit_capture",session_id:$session_id,project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"commit-capture",duration_ms:($dur|tonumber),status:$status,error:$err},memory:$mem,context:{commit_message:$commit_msg,vcs_tool:$vcs}}' \
    >> "$LOG" 2>/dev/null || true
else
  SAFE_ERROR=$(printf '%s' "$HOOK_ERROR" | tr -d '"\\')
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
