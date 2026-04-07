#!/bin/sh
# MAG pre-compact — snapshot ephemeral state before context compaction
# Fire-and-forget: no stdout contract, output is ignored by Claude Code
set -eu

STATE_DIR="$HOME/.mag/state"
LOG="$HOME/.mag/auto-capture.log"
mkdir -p "$STATE_DIR" "$HOME/.mag"

# Read stdin JSON for session context
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  TRANSCRIPT_PATH=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null) || TRANSCRIPT_PATH=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
  TRIGGER=$(printf '%s' "$INPUT" | jq -r '.trigger // empty' 2>/dev/null) || TRIGGER=""
else
  # Graceful degradation without jq
  SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
  TRANSCRIPT_PATH=""
  CWD="$PWD"
  TRIGGER=""
fi

# Fallbacks
SESSION_ID="${SESSION_ID:-${CLAUDE_SESSION_ID:-unknown}}"
CWD="${CWD:-$PWD}"
PROJECT="$(basename "$CWD")"

# Log the event
printf '%s pre_compact project=%s session=%s trigger=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" "$TRIGGER" >> "$LOG" 2>/dev/null || true

# Collect VCS state (run in the project CWD parsed from the hook payload)
VCS_STATE=""
if [ -d "$CWD" ] && command -v jj >/dev/null 2>&1 && (cd "$CWD" && jj root >/dev/null 2>&1); then
  VCS_STATE=$(cd "$CWD" && jj log --no-graph -r '@' -T 'change_id.shortest(8) ++ " " ++ description.first_line()' 2>/dev/null) || VCS_STATE=""
elif [ -d "$CWD" ] && command -v git >/dev/null 2>&1 && (cd "$CWD" && git rev-parse --git-dir >/dev/null 2>&1); then
  VCS_STATE=$(cd "$CWD" && git log --oneline -1 2>/dev/null) || VCS_STATE=""
fi

# Collect recent file from transcript
RECENT_FILE=""
if [ -n "$TRANSCRIPT_PATH" ] && [ -f "$TRANSCRIPT_PATH" ]; then
  RECENT_FILE=$(grep -oE '"file_path"[[:space:]]*:[[:space:]]*"[^"]+"' "$TRANSCRIPT_PATH" 2>/dev/null \
    | tail -1 \
    | sed 's/"file_path"[[:space:]]*:[[:space:]]*"//;s/"$//' 2>/dev/null) || RECENT_FILE=""
fi

# Build snapshot with safe JSON construction
if command -v jq >/dev/null 2>&1; then
  jq -n \
    --arg sid "$SESSION_ID" \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg proj "$PROJECT" \
    --arg cwd "$CWD" \
    --arg trigger "$TRIGGER" \
    --arg vcs "$VCS_STATE" \
    --arg rf "$RECENT_FILE" \
    '{session_id: $sid, timestamp: $ts, project: $proj, working_directory: $cwd, trigger: $trigger, vcs_state: $vcs, recent_file: $rf}' \
    > "$STATE_DIR/pre-compact-$SESSION_ID.json" 2>/dev/null || {
    printf '%s pre_compact_write_failed session=%s\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$SESSION_ID" >> "$LOG" 2>/dev/null || true
  }
else
  # Minimal fallback without jq — only safe fields (UUIDs, dirnames, timestamps)
  # vcs_state and recent_file are omitted: they can contain arbitrary characters unsafe for printf JSON
  # Sanitize CWD for safe printf JSON (remove double-quotes and backslashes)
  SAFE_CWD=$(printf '%s' "$CWD" | tr -d '"\\')
  printf '{"session_id":"%s","timestamp":"%s","project":"%s","working_directory":"%s","trigger":"%s"}\n' \
    "$SESSION_ID" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SAFE_CWD" "$TRIGGER" \
    > "$STATE_DIR/pre-compact-$SESSION_ID.json" 2>/dev/null || {
    printf '%s pre_compact_write_failed session=%s\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$SESSION_ID" >> "$LOG" 2>/dev/null || true
  }
fi

# Reap stale snapshots older than ~1 day
find "$STATE_DIR" -name 'pre-compact-*.json' -mtime +1 -delete 2>/dev/null || true

exit 0
