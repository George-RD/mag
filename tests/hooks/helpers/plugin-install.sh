#!/bin/sh
# tests/hooks/helpers/plugin-install.sh
# Usage: plugin-install.sh <CONFIG_DIR>
#
# Creates a minimal Claude Code configuration directory with hooks pointing
# to the repo's plugin/scripts/ directory using absolute paths.
# The generated settings.json is suitable for passing to `claude --settings`.

set -eu

CONFIG_DIR="${1:?Usage: plugin-install.sh <CONFIG_DIR>}"

# Resolve the repo root from this script's location:
#   this file lives at <repo>/tests/hooks/helpers/plugin-install.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT_LOCAL="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# MAG_PLUGIN_SCRIPTS_OVERRIDE takes top priority (set by run-tests.sh).
# PLUGIN_SCRIPTS_DIR is exported by common.sh (respects HOOKS_TARGET).
# Fall back to production scripts when running standalone.
SCRIPTS_DIR="${MAG_PLUGIN_SCRIPTS_OVERRIDE:-${PLUGIN_SCRIPTS_DIR:-$REPO_ROOT_LOCAL/plugin/scripts}}"

# SubagentStop hook: only dev scripts include subagent-end.sh.
# Detect presence rather than hardcoding the target name.
HAS_SUBAGENT_END=0
if [ -f "$SCRIPTS_DIR/subagent-end.sh" ]; then
  HAS_SUBAGENT_END=1
fi

# Sanity-check that the scripts directory exists
if [ ! -d "$SCRIPTS_DIR" ]; then
  printf 'plugin-install.sh: scripts dir not found: %s\n' "$SCRIPTS_DIR" >&2
  exit 1
fi

mkdir -p "$CONFIG_DIR"

# JSON-escape SCRIPTS_DIR in case the path contains quotes or backslashes
SCRIPTS_DIR_JSON="$(printf '%s' "$SCRIPTS_DIR" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"

# Build the optional SubagentStop block (dev scripts only).
SUBAGENT_BLOCK=""
if [ "$HAS_SUBAGENT_END" = "1" ]; then
  SUBAGENT_BLOCK="$(cat <<SUBEOF
    "SubagentStop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/subagent-end.sh",
            "timeout": 5000
          }
        ]
      }
    ],
SUBEOF
)"
fi

# Write settings.json with hooks using absolute paths (no CLAUDE_PLUGIN_ROOT needed)
cat > "$CONFIG_DIR/settings.json" <<EOF
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/session-start.sh",
            "timeout": 5000
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/prompt-gate.sh",
            "timeout": 200
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/pre-compact.sh",
            "timeout": 2000
          }
        ]
      }
    ],
    "PostCompact": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/compact-refresh.sh",
            "timeout": 3000
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/commit-capture.sh",
            "timeout": 4000
          },
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/error-capture.sh",
            "timeout": 4000
          }
        ]
      }
    ],
$SUBAGENT_BLOCK
    "Stop": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "$SCRIPTS_DIR_JSON/session-end.sh",
            "timeout": 5000
          }
        ]
      }
    ]
  }
}
EOF

printf 'plugin-install: wrote settings.json to %s\n' "$CONFIG_DIR" >&2
