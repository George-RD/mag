#!/bin/sh
# MAG post-compact — re-inject memories + restore pre-compact state
set -eu

LOG="$HOME/.mag/auto-capture.log"
STATE_DIR="$HOME/.mag/state"
mkdir -p "$HOME/.mag" "$STATE_DIR"

# Read stdin JSON for session context
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
  COMPACT_SUMMARY=$(printf '%s' "$INPUT" | jq -r '.compact_summary // empty' 2>/dev/null) || COMPACT_SUMMARY=""
else
  SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
  CWD="$PWD"
  COMPACT_SUMMARY=""
fi

SESSION_ID="${SESSION_ID:-${CLAUDE_SESSION_ID:-unknown}}"
CWD="${CWD:-$PWD}"
PROJECT="$(basename "$CWD")"

# Log the event
printf '%s compact_refresh project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

# Re-inject top memories (full budget — context is smallest after compaction)
mag welcome --project "$PROJECT" --session-id "$SESSION_ID" --budget-tokens 2000 2>/dev/null || true

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
if [ -f "$SNAPSHOT" ] && command -v jq >/dev/null 2>&1; then
  WD=$(jq -r '.working_directory // empty' "$SNAPSHOT" 2>/dev/null) || WD=""
  RF=$(jq -r '.recent_file // empty' "$SNAPSHOT" 2>/dev/null) || RF=""
  VCS=$(jq -r '.vcs_state // empty' "$SNAPSHOT" 2>/dev/null) || VCS=""

  RESTORE=""
  [ -n "$WD" ] && RESTORE="${RESTORE}Working directory: $WD\\n"
  [ -n "$RF" ] && RESTORE="${RESTORE}Recent file: $RF\\n"
  [ -n "$VCS" ] && RESTORE="${RESTORE}VCS state: $VCS\\n"

  if [ -n "$RESTORE" ]; then
    printf '{"additionalContext":"<MAG_RESTORE>Pre-compact state recovered:\\n%s</MAG_RESTORE>"}\n' "$RESTORE"
  fi

  rm -f "$SNAPSHOT"
fi

exit 0
