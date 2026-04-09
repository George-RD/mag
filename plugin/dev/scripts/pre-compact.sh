#!/bin/sh
# MAG dev pre-compact — snapshot ephemeral state before context compaction
# Fire-and-forget: no stdout contract, output is ignored by Claude Code
set -eu

MAG_DATA_ROOT="$HOME/.dev-mag"
export MAG_DATA_ROOT

STATE_DIR="$MAG_DATA_ROOT/state"
LOG="$MAG_DATA_ROOT/auto-capture.jsonl"
# Millisecond-precision timestamp (perl is POSIX-portable; date +%s%N is Linux-only)
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
START_TS=$(now_ms)
mkdir -p "$MAG_DATA_ROOT" "$STATE_DIR"

# Read stdin JSON for session context
INPUT=$(cat 2>/dev/null) || INPUT=""

if command -v jq >/dev/null 2>&1 && [ -n "$INPUT" ]; then
  SESSION_ID=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null) || SESSION_ID=""
  TRANSCRIPT_PATH=$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null) || TRANSCRIPT_PATH=""
  CWD=$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null) || CWD=""
  TRIGGER=$(printf '%s' "$INPUT" | jq -r '.trigger // empty' 2>/dev/null) || TRIGGER=""
else
  # Graceful degradation without jq
  SESSION_ID="${CLAUDE_SESSION_ID:-}"
  TRANSCRIPT_PATH=""
  CWD="$PWD"
  TRIGGER=""
fi

# Fallbacks — explicit defaults for all variables used under set -u
SESSION_ID="${SESSION_ID:-${CLAUDE_SESSION_ID:-}}"
CWD="${CWD:-$PWD}"
TRIGGER="${TRIGGER:-}"
TRANSCRIPT_PATH="${TRANSCRIPT_PATH:-}"
PROJECT="$(basename "$CWD")"

# Collect VCS state (run in the project CWD parsed from the hook payload)
VCS_STATE=""
if [ -d "$CWD" ] && command -v jj >/dev/null 2>&1 && (cd "$CWD" && jj root >/dev/null 2>&1); then
  VCS_STATE=$(cd "$CWD" && jj log --no-graph -r '@' -T 'change_id.shortest(8) ++ " " ++ description.first_line()' 2>/dev/null) || VCS_STATE=""
elif [ -d "$CWD" ] && command -v jj >/dev/null 2>&1; then
  VCS_STATE=$(cd "$CWD" && jj log --oneline -1 2>/dev/null) || VCS_STATE=""
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
    > "$STATE_DIR/pre-compact-$SESSION_ID.json" 2>/dev/null || true
else
  # Minimal fallback without jq
  SAFE_CWD=$(printf '%s' "$CWD" | tr -d '"\\')
  printf '{"session_id":"%s","timestamp":"%s","project":"%s","working_directory":"%s","trigger":"%s"}\n' \
    "$SESSION_ID" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SAFE_CWD" "$TRIGGER" \
    > "$STATE_DIR/pre-compact-$SESSION_ID.json" 2>/dev/null || true
fi

END_TS=$(now_ms)
DURATION_MS=$(( END_TS - START_TS ))

# Emit JSONL — schema v:0 (campaign decision D1)
if command -v jq >/dev/null 2>&1; then
  if [ -n "$SESSION_ID" ]; then SID_JSON="\"$SESSION_ID\""; else SID_JSON="null"; fi
  jq -n \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson session_id "$SID_JSON" \
    --arg proj "$PROJECT" \
    --arg dur "$DURATION_MS" \
    --arg vcs "${VCS_STATE:-}" \
    --arg rf "${RECENT_FILE:-}" \
    --arg trigger "${TRIGGER:-}" \
    --arg transcript "${TRANSCRIPT_PATH:-}" \
    '{v:0,ts:$ts,event:"hook.pre_compact",session_id:$session_id,project:$proj,agent:{id:null,type:null,tool:"claude_code"},hook:{name:"pre-compact",duration_ms:($dur|tonumber),status:"ok",error:null},memory:null,context:{vcs_state:$vcs,recent_file:$rf,trigger:$trigger,transcript_path:$transcript}}' \
    >> "$LOG" 2>/dev/null || true
else
  # Fallback — also v:0 for schema consistency
  printf '{"v":0,"ts":"%s","event":"hook.pre_compact","session_id":null,"project":"%s","hook":{"name":"pre-compact","duration_ms":%s,"status":"ok","error":null}}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$DURATION_MS" \
    >> "$LOG" 2>/dev/null || true
fi

# Reap stale snapshots older than ~1 day
find "$STATE_DIR" -name 'pre-compact-*.json' -mtime +1 -delete 2>/dev/null || true

exit 0
