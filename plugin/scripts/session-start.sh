#!/bin/sh
# MAG session start — recall project context
# Outputs memory context for Claude Code injection
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Log BEFORE invocation so failed mag calls are still recorded
mkdir -p "$HOME/.mag"
printf '%s session_start project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

mag welcome --project "$PROJECT" --session-id "$SESSION_ID" --budget-tokens 2000 2>/dev/null || true
