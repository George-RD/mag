#!/bin/sh
# plugin/dev/test-env.sh
# Creates an isolated MAG dev plugin test environment.
#
# Usage:
#   ./test-env.sh [--clone] [--no-session] [--teardown]
#
# Flags:
#   --clone       Sync production ~/.mag/memory.db to ~/.dev-mag/ before setup
#   --no-session  Skip launching the terminal session (just set up the repo)
#   --teardown    Destroy the test repo and ~/.dev-mag, kill the session

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEST_REPO="${MAG_TEST_REPO:-/tmp/mag-test-repo}"
DEV_ROOT="$HOME/.dev-mag"
SESSION_NAME="mag-test"

CLONE=0
NO_SESSION=0
TEARDOWN=0

for _arg in "$@"; do
  case "$_arg" in
    --clone)      CLONE=1 ;;
    --no-session) NO_SESSION=1 ;;
    --teardown)   TEARDOWN=1 ;;
    *) printf 'test-env.sh: unknown argument: %s\n' "$_arg" >&2; exit 1 ;;
  esac
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_have() { command -v "$1" >/dev/null 2>&1; }

_tmux_kill() {
  if _have tmux; then
    tmux kill-session -t "$SESSION_NAME" 2>/dev/null || true
  fi
}

# ---------------------------------------------------------------------------
# Teardown
# ---------------------------------------------------------------------------

if [ "$TEARDOWN" -eq 1 ]; then
  printf '==> Tearing down test environment\n'

  _tmux_kill

  if [ -d "$TEST_REPO" ]; then
    printf '    Removing %s ...\n' "$TEST_REPO"
    rm -rf "$TEST_REPO"
    printf '    OK\n'
  else
    printf '    %s not found — skipping\n' "$TEST_REPO"
  fi

  if [ -d "$DEV_ROOT" ]; then
    printf '    Removing %s ...\n' "$DEV_ROOT"
    rm -rf "$DEV_ROOT"
    printf '    OK\n'
  else
    printf '    %s not found — skipping\n' "$DEV_ROOT"
  fi

  printf '==> Teardown complete\n'
  exit 0
fi

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------

printf '==> MAG dev plugin test environment setup\n'
printf '    Test repo : %s\n' "$TEST_REPO"
printf '    Dev root  : %s\n' "$DEV_ROOT"
printf '\n'

# 1. Create test repo
printf -- '--> Creating test repo at %s ...\n' "$TEST_REPO"
mkdir -p "$TEST_REPO"
if [ ! -d "$TEST_REPO/.git" ]; then
  git -C "$TEST_REPO" init --quiet
  printf '    Initialised git repo\n'
else
  printf '    Already a git repo — skipping init\n'
fi

# 2. Run setup.sh (creates ~/.dev-mag, verifies deps, renders .mcp.json)
printf -- '--> Running setup.sh ...\n'
if [ "$CLONE" -eq 1 ]; then
  sh "$SCRIPT_DIR/setup.sh" --clone
else
  sh "$SCRIPT_DIR/setup.sh"
fi

# 3. Write per-repo settings.local.json scoping dev plugin to this test repo only
printf -- '--> Writing %s/.claude/settings.local.json ...\n' "$TEST_REPO"
mkdir -p "$TEST_REPO/.claude"

# JSON-escape the absolute plugin path
_plugin_path_json="$(printf '%s' "$SCRIPT_DIR" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"

cat > "$TEST_REPO/.claude/settings.local.json" <<EOF
{
  "plugins": [
    {"path": "$_plugin_path_json"}
  ]
}
EOF
printf '    Written\n'

# ---------------------------------------------------------------------------
# Session launch
# ---------------------------------------------------------------------------

if [ "$NO_SESSION" -eq 1 ]; then
  printf '\n==> Setup complete (--no-session: skipping terminal launch)\n'
  printf '\n'
  printf 'To start manually:\n'
  printf '  cd %s && claude\n' "$TEST_REPO"
  printf '\n'
  printf 'To tail telemetry:\n'
  printf '  tail -f %s/auto-capture.jsonl | jq .\n' "$DEV_ROOT"
  printf '\n'
  exit 0
fi

# Determine terminal multiplexer
MULTIPLEXER=""
if _have cmux; then
  MULTIPLEXER="cmux"
elif _have tmux; then
  MULTIPLEXER="tmux"
fi

# Pane commands
PANE1_CMD="cd '$TEST_REPO' && exec \$SHELL"
PANE2_CMD="tail -f '$DEV_ROOT/auto-capture.jsonl' | jq ."

case "$MULTIPLEXER" in
  cmux|tmux)
    # Kill any existing session first
    _tmux_kill

    # Both cmux and tmux share the same session/window/pane API
    printf -- '--> Launching %s session "%s" ...\n' "$MULTIPLEXER" "$SESSION_NAME"

    # Create detached session with pane 1 in the test repo
    "$MULTIPLEXER" new-session -d -s "$SESSION_NAME" -x 220 -y 50 \; \
      send-keys "cd '$TEST_REPO'" Enter \; \
      split-window -h \; \
      send-keys "$PANE2_CMD" Enter \; \
      select-pane -t 0

    printf '    Session "%s" created\n' "$SESSION_NAME"
    printf '\n'
    printf 'Attach with:\n'
    printf '  %s attach -t %s\n' "$MULTIPLEXER" "$SESSION_NAME"
    ;;

  *)
    printf '\n==> Setup complete (cmux/tmux not found — run these manually):\n'
    printf '\n'
    printf '# Pane 1 — run claude in the test repo:\n'
    printf '  cd %s && claude\n' "$TEST_REPO"
    printf '\n'
    printf '# Pane 2 — watch JSONL telemetry:\n'
    printf '  tail -f %s/auto-capture.jsonl | jq .\n' "$DEV_ROOT"
    printf '\n'
    ;;
esac

printf '\n==> Done\n'
