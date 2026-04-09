#!/bin/sh
# MAG dev plugin setup
# Usage: ./setup.sh [--clone] [--force]
#   --clone  Copy production ~/.mag/memory.db to ~/.dev-mag/ for testing with real data
#   --force  Overwrite .mcp.json even if it already exists
#
# NOTE: mcp.json is a TEMPLATE — do NOT edit .mcp.json directly.
# setup.sh processes mcp.json into .mcp.json by expanding $HOME.
# If .mcp.json is missing or stale, re-run this script (or use --force).
set -eu

CLONE=0
FORCE=0
for arg in "$@"; do
  case "$arg" in
    --clone) CLONE=1 ;;
    --force) FORCE=1 ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done

DEV_ROOT="$HOME/.dev-mag"
PROD_ROOT="$HOME/.mag"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> MAG dev plugin setup"
echo "    Dev data root: $DEV_ROOT"
echo ""

# 1. Create dev data directory
echo "--> Creating $DEV_ROOT ..."
mkdir -p "$DEV_ROOT"
mkdir -p "$DEV_ROOT/state"
echo "    OK"

# 2. Optionally clone production DB
if [ "$CLONE" -eq 1 ]; then
  if [ -f "$PROD_ROOT/memory.db" ]; then
    echo "--> Cloning $PROD_ROOT/memory.db to $DEV_ROOT/memory.db ..."
    # SQLite uses WAL mode — use sqlite3 backup API for consistency, fall back to
    # copying the DB + WAL + SHM sidecars together if sqlite3 is not available.
    if command -v sqlite3 >/dev/null 2>&1; then
      sqlite3 "$PROD_ROOT/memory.db" ".backup '$DEV_ROOT/memory.db'"
      echo "    OK (atomic backup via sqlite3)"
    else
      cp "$PROD_ROOT/memory.db" "$DEV_ROOT/memory.db"
      [ -f "$PROD_ROOT/memory.db-wal" ] && cp "$PROD_ROOT/memory.db-wal" "$DEV_ROOT/memory.db-wal" || true
      [ -f "$PROD_ROOT/memory.db-shm" ] && cp "$PROD_ROOT/memory.db-shm" "$DEV_ROOT/memory.db-shm" || true
      echo "    OK ($(du -sh "$DEV_ROOT/memory.db" | cut -f1) copied; sqlite3 not found — copied sidecars too)"
    fi
  else
    echo "    WARNING: $PROD_ROOT/memory.db not found — skipping clone"
  fi
fi

# 3. Verify mag CLI is available
echo "--> Checking mag CLI ..."
if ! command -v mag >/dev/null 2>&1; then
  echo "    ERROR: 'mag' not found in PATH. Install it first:"
  echo "           cargo install --git https://github.com/George-RD/mag"
  exit 1
fi
MAG_VERSION=$(mag --version 2>/dev/null | head -1 || echo "unknown")
echo "    Found: $MAG_VERSION"

# 4. Verify jq is available (needed for JSONL output)
echo "--> Checking jq (required for full telemetry) ..."
if command -v jq >/dev/null 2>&1; then
  echo "    Found: $(jq --version)"
else
  echo "    WARNING: jq not found — JSONL telemetry will run in degraded mode."
  echo "             Some fields (agent, context sub-fields, memory block) will be omitted."
  echo "             Install jq for full telemetry: https://jqlang.github.io/jq/download/"
fi

# 5. Make all hook scripts executable
echo "--> Setting script permissions ..."
chmod +x "$SCRIPT_DIR/scripts/"*.sh
echo "    OK"

# 6. Install the .mcp.json with $HOME expanded
# Note: .mcp.json shipped as mcp.json because dotfiles can't be committed in some envs.
# setup.sh installs it to the correct location, always regenerating to pick up any changes.
MCP_SRC="$SCRIPT_DIR/mcp.json"
MCP_DEST="$SCRIPT_DIR/.mcp.json"
if [ -f "$MCP_SRC" ]; then
  # Always re-render so template changes are picked up on every run.
  # (--force is kept for backward compat but is now a no-op gate.)
  echo "--> Rendering .mcp.json from template ..."
  # Expand $HOME in the env value (mcp.json is a template; .mcp.json is the live file)
  sed "s|\${HOME}|$HOME|g" "$MCP_SRC" > "$MCP_DEST"
  # Verify expansion succeeded — .mcp.json must not contain literal ${HOME} or $HOME
  if grep -qE '\$\{?HOME\}?' "$MCP_DEST" 2>/dev/null; then
    echo "    ERROR: .mcp.json still contains literal \$HOME — expansion failed" >&2
    rm -f "$MCP_DEST"
    exit 1
  fi
  echo "    Installed at $MCP_DEST"
else
  echo "    WARNING: mcp.json source not found — skipping .mcp.json install"
fi

# 7. Print installation instructions
cat << EOF

==> Installation complete!

To install the mag-dev plugin in Claude Code:

  mag plugin install "$SCRIPT_DIR" --name mag-dev

Or add it to your local marketplace:

  mkdir -p "\$HOME/.claude/plugins/local"
  ln -sf "$SCRIPT_DIR" "\$HOME/.claude/plugins/local/mag-dev"
  mag plugin install local:mag-dev

To verify isolation is working:

  MAG_DATA_ROOT="$DEV_ROOT" mag health

To tail the JSONL telemetry log:

  tail -f "$DEV_ROOT/auto-capture.jsonl" | jq .

To remove the dev environment:

  rm -rf "$DEV_ROOT"

EOF
