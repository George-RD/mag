#!/bin/sh
# MAG session end — store lightweight session summary
PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
mag hook session-end --project "$PROJECT" --session-id "$SESSION_ID" 2>/dev/null || true
