#!/bin/sh
# MAG session end — store lightweight session summary
# Stop hook: $CLAUDE_TRANSCRIPT may be set by Claude Code
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Build summary from transcript tail if available; fall back to minimal marker
if [ -n "${CLAUDE_TRANSCRIPT:-}" ]; then
  # Truncate to 500 bytes to keep memory concise and avoid storing secrets
  TAIL="$(printf '%.500s' "$CLAUDE_TRANSCRIPT")"
  SUMMARY="Session ended. Project: $PROJECT. Recent context: $TAIL"
else
  SUMMARY="Session ended. Project: $PROJECT."
fi

# Log BEFORE invocation
mkdir -p "$HOME/.mag"
printf '%s session_end project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

# Use session_end event type (TTL_LONG_TERM = 14 days).
# Do NOT use session_summary: that maps to TTL_EPHEMERAL (1 hour) and self-destructs.
mag process "$SUMMARY" \
  --event-type session_end \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.4 2>/dev/null || true
