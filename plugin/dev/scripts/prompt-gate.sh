#!/bin/sh
# MAG dev prompt gate — pure regex, NO daemon call, <1ms
# Outputs a hint only when the prompt suggests memory would help
# Emits JSONL to log only when a hint is emitted (keeps log clean)
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

LOG="$MAG_DATA_ROOT/auto-capture.jsonl"

# Read stdin JSON (UserPromptSubmit provides prompt field)
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  PROMPT=$(printf '%s' "$INPUT" | jq -r '.prompt // empty' 2>/dev/null) || PROMPT=""
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
else
  # Fallback: read first line as raw text (legacy behavior)
  PROMPT=$(printf '%s' "$INPUT" | head -1 || true)
  SESSION_ID=""
fi

SESSION_ID="${SESSION_ID:-}"

# Check for memory-relevant signals
HINT=""
HINT_TYPE=""
case "$PROMPT" in
  *remember*|*"don't forget"*|*"store this"*|*"note that"*|*"save this"*)
    HINT='{"additionalContext":"<MAG_HINT>User wants to store something. Use: mag process \"content\" --event-type TYPE --project PROJECT --importance 0.8</MAG_HINT>"}'
    HINT_TYPE="store"
    ;;
  *"last time"*|*previously*|*"we discussed"*|*"what did we"*|*"we decided"*|*recall*)
    HINT='{"additionalContext":"<MAG_HINT>User references prior context. Use: mag advanced-search \"query\" --project PROJECT --limit 10</MAG_HINT>"}'
    HINT_TYPE="recall"
    ;;
  *checkpoint*|*handoff*|*"wrap up"*|*"pick up where"*)
    HINT='{"additionalContext":"<MAG_HINT>User wants checkpoint/handoff. Consider using mag checkpoint \"title\" \"progress\" --project PROJECT</MAG_HINT>"}'
    HINT_TYPE="checkpoint"
    ;;
  *)
    # Default: silence. No memory injection needed.
    exit 0
    ;;
esac

# Emit the additionalContext for Claude Code
printf '%s\n' "$HINT"

# Log JSONL event (only when hint emitted)
mkdir -p "$MAG_DATA_ROOT"
PROJECT="$(basename "$PWD")"
PROMPT_PREVIEW="$(printf '%.80s' "$PROMPT")"
if command -v jq >/dev/null 2>&1; then
  # Use --arg for session_id (not --argjson) so special chars in session IDs are safe
  jq -nc \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg session_id "$SESSION_ID" \
    --arg proj "$PROJECT" \
    --arg prompt_preview "$PROMPT_PREVIEW" \
    --arg hint_type "$HINT_TYPE" \
    '{v:0,ts:$ts,event:"hook.prompt_gate",session_id:($session_id | if . == "" then null else . end),project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"prompt-gate",duration_ms:0,status:"ok",error:null},memory:null,context:{prompt_preview:$prompt_preview,hint_type:$hint_type}}' \
    >> "$LOG" 2>/dev/null || true
else
  printf '{"v":0,"ts":"%s","event":"hook.prompt_gate","session_id":null,"project":"%s","hook":{"name":"prompt-gate","duration_ms":0,"status":"ok","error":null}}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" \
    >> "$LOG" 2>/dev/null || true
fi

exit 0
