#!/bin/sh
# MAG commit-capture — auto-capture jj/git commit messages as Decision memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (JSON), $CLAUDE_TOOL_OUTPUT (JSON)
set -eu

# Fast-path rejection — plain string check before any process forks.
# CLAUDE_TOOL_INPUT is JSON like {"command":"jj commit -m ..."}, so a substring
# match on the raw string is safe and avoids the cost of jq for ~95% of calls.
INPUT="${CLAUDE_TOOL_INPUT:-}"
case "$INPUT" in
  *"jj commit"*|*"jj describe"*|*"git commit"*) ;;
  *) exit 0 ;;
esac

COMMAND="$(printf '%s' "$INPUT" | jq -r '.command // empty' 2>/dev/null || true)"

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
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"

mkdir -p "$HOME/.mag"
printf '%s git_commit project=%s session=%s msg=%.80s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" "$MSG" \
  >> "$HOME/.mag/auto-capture.log" 2>/dev/null || true

mag process "Commit: $MSG" \
  --event-type git_commit \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 2>/dev/null || true
