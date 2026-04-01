#!/bin/sh
# MAG post-compact — re-inject top memories after context compaction
# Budget trimming deferred to Wave 2 (requires --budget-tokens flag, not yet in Rust)
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

mkdir -p "$HOME/.mag"
printf '%s compact_refresh project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

mag welcome --project "$PROJECT" --session-id "$SESSION_ID" 2>/dev/null || true
