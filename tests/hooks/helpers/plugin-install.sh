#!/bin/sh
# tests/hooks/helpers/plugin-install.sh
# Usage: plugin-install.sh <CONFIG_DIR>
#
# Creates a minimal Claude Code configuration directory with hooks pointing
# to the repo's plugin/scripts/ directory using absolute paths.
# The generated settings.json is suitable for passing to `claude --config-dir`.

set -eu

CONFIG_DIR="${1:?Usage: plugin-install.sh <CONFIG_DIR>}"

# Resolve the repo root from this script's location:
#   this file lives at <repo>/tests/hooks/helpers/plugin-install.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
SCRIPTS_DIR="${MAG_PLUGIN_SCRIPTS_OVERRIDE:-$REPO_ROOT/plugin/scripts}"

# Sanity-check that the scripts directory exists
if [ ! -d "$SCRIPTS_DIR" ]; then
  printf 'plugin-install.sh: scripts dir not found: %s\n' "$SCRIPTS_DIR" >&2
  exit 1
fi

mkdir -p "$CONFIG_DIR"

# JSON-escape SCRIPTS_DIR in case the path contains quotes or backslashes
SCRIPTS_DIR_JSON="$(printf '%s' "$SCRIPTS_DIR" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"

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
