#!/bin/sh
# tests/hooks/helpers/common.sh — shared helpers for hook integration tests
# Source this file at the top of every test: . "$(dirname "$0")/helpers/common.sh"
# POSIX sh only — no bashisms.

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
CLAUDE_MODEL="${CLAUDE_MODEL:-haiku}"

# Absolute path to the plugin scripts directory in this repo.
# Resolve relative to this file's own location so tests work regardless of cwd.
_HELPERS_DIR="$(cd "$(dirname "$0")/helpers" 2>/dev/null && pwd)"
if [ -z "$_HELPERS_DIR" ]; then
  # Fallback: resolve from __file__ via $0 (caller's $0 not this file's)
  _HELPERS_DIR="$(cd "$(dirname "$0")/helpers" && pwd)"
fi
# Walk up two levels from helpers/ to repo root
REPO_ROOT="$(cd "$_HELPERS_DIR/../../.." && pwd)"

# ---------------------------------------------------------------------------
# HOOKS_TARGET — select production (default) or dev plugin
#
# Usage:
#   HOOKS_TARGET=dev sh tests/hooks/run_all.sh   # use plugin/dev/scripts/
#   sh tests/hooks/run_all.sh                    # use plugin/scripts/ (default)
#
# Dev mode also sets MAG_DATA_ROOT=~/.dev-mag so hooks write to the dev store.
# ---------------------------------------------------------------------------
HOOKS_TARGET="${HOOKS_TARGET:-production}"

case "$HOOKS_TARGET" in
  dev)
    MAG_DATA_ROOT="${MAG_DATA_ROOT:-$HOME/.dev-mag}"
    PLUGIN_SCRIPTS_DIR="$REPO_ROOT/plugin/dev/scripts"
    ;;
  production)
    MAG_DATA_ROOT="${MAG_DATA_ROOT:-$HOME/.mag}"
    PLUGIN_SCRIPTS_DIR="$REPO_ROOT/plugin/scripts"
    ;;
  *)
    printf 'common.sh: unknown HOOKS_TARGET=%s (must be "production" or "dev")\n' \
      "$HOOKS_TARGET" >&2
    exit 2
    ;;
esac

JSONL_LOG="$MAG_DATA_ROOT/auto-capture.jsonl"

export HOOKS_TARGET MAG_DATA_ROOT JSONL_LOG CLAUDE_MODEL PLUGIN_SCRIPTS_DIR REPO_ROOT

# ---------------------------------------------------------------------------
# State tracking (set by setup_test)
# ---------------------------------------------------------------------------
CURRENT_TEST_NAME=""
JSONL_MARK=0
MAG_MEMORY_COUNT_BEFORE=0
TEST_TMPDIR=""

# ---------------------------------------------------------------------------
# setup_test <test_name>
#   Create per-test TMPDIR, snapshot log position, snapshot memory count.
# ---------------------------------------------------------------------------
setup_test() {
  CURRENT_TEST_NAME="${1:-unknown}"
  TEST_TMPDIR="$(mktemp -d)"
  export TEST_TMPDIR

  # Snapshot current line count in the JSONL log (may not exist yet)
  mkdir -p "$MAG_DATA_ROOT"
  if [ -f "$JSONL_LOG" ]; then
    JSONL_MARK="$(wc -l < "$JSONL_LOG" | tr -d ' ')"
  else
    JSONL_MARK=0
  fi

  # Snapshot current memory count
  MAG_MEMORY_COUNT_BEFORE="$(_mag_list_count)"

  export CURRENT_TEST_NAME JSONL_MARK MAG_MEMORY_COUNT_BEFORE
}

# ---------------------------------------------------------------------------
# teardown_test
#   Remove per-test TMPDIR.
# ---------------------------------------------------------------------------
teardown_test() {
  if [ -n "$TEST_TMPDIR" ] && [ -d "$TEST_TMPDIR" ]; then
    rm -rf "$TEST_TMPDIR"
  fi
  TEST_TMPDIR=""
}

# ---------------------------------------------------------------------------
# get_new_jsonl_lines
#   Print all JSONL lines appended to the log since setup_test was called.
# ---------------------------------------------------------------------------
get_new_jsonl_lines() {
  if [ ! -f "$JSONL_LOG" ]; then
    return 0
  fi
  # tail -n +N prints from line N; JSONL_MARK is the count BEFORE the test,
  # so new lines start at JSONL_MARK+1.
  _start=$(( JSONL_MARK + 1 ))
  tail -n +"$_start" "$JSONL_LOG"
}

# ---------------------------------------------------------------------------
# get_event <event_name>
#   Filter new JSONL lines to those where .event == event_name.
# ---------------------------------------------------------------------------
get_event() {
  _event="$1"
  get_new_jsonl_lines | jq -c --arg ev "$_event" 'select(.event == $ev)' 2>/dev/null
}

# ---------------------------------------------------------------------------
# assert_event_fired <event_name>
#   Fail if no new JSONL line has .event == event_name.
# ---------------------------------------------------------------------------
assert_event_fired() {
  _ev="$1"
  _result="$(get_event "$_ev")"
  if [ -z "$_result" ]; then
    fail "Expected event '$_ev' to be fired, but found none in new JSONL lines"
  fi
}

# ---------------------------------------------------------------------------
# assert_jsonl_field <event_name> <jq_path> <expected>
#   Extract field via jq from the matching event and compare to expected.
# ---------------------------------------------------------------------------
assert_jsonl_field() {
  _ev="$1"
  _path="$2"
  _expected="$3"
  _line="$(get_event "$_ev" | head -1)"
  if [ -z "$_line" ]; then
    fail "assert_jsonl_field: no event '$_ev' found"
  fi
  _actual="$(printf '%s' "$_line" | jq -r "$_path" 2>/dev/null)"
  if [ "$_actual" != "$_expected" ]; then
    fail "assert_jsonl_field($_ev, $_path): expected '$_expected', got '$_actual'"
  fi
}

# ---------------------------------------------------------------------------
# assert_jsonl_field_nonempty <event_name> <jq_path>
#   Fail if the extracted field is empty or null.
# ---------------------------------------------------------------------------
assert_jsonl_field_nonempty() {
  _ev="$1"
  _path="$2"
  _line="$(get_event "$_ev" | head -1)"
  if [ -z "$_line" ]; then
    fail "assert_jsonl_field_nonempty: no event '$_ev' found"
  fi
  _actual="$(printf '%s' "$_line" | jq -r "$_path" 2>/dev/null)"
  if [ -z "$_actual" ] || [ "$_actual" = "null" ]; then
    fail "assert_jsonl_field_nonempty($_ev, $_path): field is empty or null"
  fi
}

# ---------------------------------------------------------------------------
# assert_memory_stored <min_count>
#   Fail if the number of new memories stored is below min_count.
# ---------------------------------------------------------------------------
assert_memory_stored() {
  _min="${1:-1}"
  _after="$(_mag_list_count)"
  _delta=$(( _after - MAG_MEMORY_COUNT_BEFORE ))
  if [ "$_delta" -lt "$_min" ]; then
    fail "assert_memory_stored: expected at least $_min new memories, got $_delta"
  fi
}

# ---------------------------------------------------------------------------
# pass <msg>
# ---------------------------------------------------------------------------
pass() {
  printf 'ok — %s: %s\n' "$CURRENT_TEST_NAME" "${1:-passed}"
}

# ---------------------------------------------------------------------------
# fail <msg>
# ---------------------------------------------------------------------------
fail() {
  printf 'FAIL — %s: %s\n' "$CURRENT_TEST_NAME" "${1:-failed}" >&2
  teardown_test
  exit 1
}

# ---------------------------------------------------------------------------
# skip_test <reason>
#   Exit 77 (TAP skip convention; also used by automake).
# ---------------------------------------------------------------------------
skip_test() {
  printf 'SKIP — %s: %s\n' "$CURRENT_TEST_NAME" "${1:-skipped}"
  teardown_test
  exit 77
}

# ---------------------------------------------------------------------------
# run_claude <prompt> [extra_flags...]
#   Invoke claude -p with standard flags for hook integration tests.
#   Extra flags are appended verbatim.
# ---------------------------------------------------------------------------
run_claude() {
  _prompt="$1"
  shift || true

  # Claude Code loads hooks from <cwd>/.claude/settings.json (project settings),
  # NOT from --settings <file>. The --settings flag merges extra config but does
  # NOT trigger hook discovery. We must place hooks in .claude/settings.json
  # inside the working directory we pass to claude.
  #
  # Additionally, --max-budget-usd causes claude to exit with a non-zero error
  # status ("Exceeded USD budget") before the Stop hook fires. Remove it so
  # the Stop/SubagentStop events are always captured.
  _run_dir="$(mktemp -d)"
  mkdir -p "$_run_dir/.claude"
  "$REPO_ROOT/tests/hooks/helpers/plugin-install.sh" "$_run_dir/.claude"

  # Rename the generated file to settings.json inside .claude/
  # plugin-install.sh writes to <DIR>/settings.json, so it's already there.

  ( cd "$_run_dir" && claude \
    -p "$_prompt" \
    --model "$CLAUDE_MODEL" \
    --dangerously-skip-permissions \
    --max-turns 3 \
    "$@" 2>/dev/null ) || true

  rm -rf "$_run_dir"
}

# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

# _mag_list_count: emit count of stored memories via mag list --json
_mag_list_count() {
  _cnt=0
  if command -v mag >/dev/null 2>&1; then
    _cnt="$(mag list --json 2>/dev/null | jq 'length' 2>/dev/null || printf '0')"
  fi
  printf '%s' "${_cnt:-0}"
}
