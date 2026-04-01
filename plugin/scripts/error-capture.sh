#!/bin/sh
# MAG error-capture — auto-capture build/test failures as error_pattern memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (command JSON), $CLAUDE_TOOL_OUTPUT (output JSON)
set -eu

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
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Telemetry: log BEFORE mag invocation
# Note: hooks in the same array run sequentially, so no flock needed vs commit-capture
mkdir -p "$HOME/.mag"
printf '%s [error-capture] project=%s error=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date +%s)" \
  "$PROJECT" \
  "$ERROR_LINE" \
  >> "$LOG" 2>/dev/null || true

mag process "Build/test error in $PROJECT: $ERROR_LINE" \
  --event-type error_pattern \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 \
  2>/dev/null || true
