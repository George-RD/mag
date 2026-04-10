#!/bin/sh
# plugin/dev/run-tests.sh
# Thin wrapper around tests/hooks/run_all.sh for the dev plugin test suite.
#
# Usage:
#   ./run-tests.sh [--filter <glob>] [--model <name>]
#
# Flags:
#   --filter <glob>   Pass-through glob filter to run_all.sh (e.g. t01)
#   --model <name>    Claude model to use (default: haiku)
#
# Exports to run_all.sh:
#   MAG_DATA_ROOT                   — points to ~/.dev-mag
#   CLAUDE_MODEL                    — the chosen model
#   MAG_PLUGIN_SCRIPTS_OVERRIDE     — redirects hook scripts to plugin/dev/scripts/

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# Resolve repo root: plugin/dev/ is two levels below the repo root
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEV_ROOT="$HOME/.dev-mag"
FILTER=""
MODEL="haiku"

# Parse flags
while [ "$#" -gt 0 ]; do
  case "$1" in
    --filter)
      if [ -z "${2:-}" ]; then
        printf 'run-tests.sh: --filter requires an argument\n' >&2
        exit 1
      fi
      FILTER="$2"
      shift 2
      ;;
    --model)
      if [ -z "${2:-}" ]; then
        printf 'run-tests.sh: --model requires an argument\n' >&2
        exit 1
      fi
      MODEL="$2"
      shift 2
      ;;
    *)
      printf 'run-tests.sh: unknown argument: %s\n' "$1" >&2
      exit 1
      ;;
  esac
done

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

_have() { command -v "$1" >/dev/null 2>&1; }
_missing=0

if ! _have mag; then
  printf 'run-tests.sh: ERROR: mag CLI not found in PATH\n' >&2
  _missing=1
fi

if ! _have claude; then
  printf 'run-tests.sh: ERROR: claude CLI not found in PATH\n' >&2
  _missing=1
fi

if ! _have jq; then
  printf 'run-tests.sh: ERROR: jq not found in PATH\n' >&2
  _missing=1
fi

if [ "$_missing" -gt 0 ]; then
  exit 1
fi

# ---------------------------------------------------------------------------
# Run the test suite
# ---------------------------------------------------------------------------

RUN_ALL="$REPO_ROOT/tests/hooks/run_all.sh"
if [ ! -f "$RUN_ALL" ]; then
  printf 'run-tests.sh: ERROR: run_all.sh not found at %s\n' "$RUN_ALL" >&2
  exit 1
fi

TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
TAP_LOG="$DEV_ROOT/test-results-$TIMESTAMP.tap"
mkdir -p "$DEV_ROOT"

printf '==> MAG dev plugin test runner\n'
printf '    Model   : %s\n' "$MODEL"
printf '    Scripts : %s/scripts\n' "$SCRIPT_DIR"
printf '    Data    : %s\n' "$DEV_ROOT"
printf '    Log     : %s\n' "$TAP_LOG"
if [ -n "$FILTER" ]; then
  printf '    Filter  : %s\n' "$FILTER"
fi
printf '\n'

export MAG_DATA_ROOT="$DEV_ROOT"
export CLAUDE_MODEL="$MODEL"
export MAG_PLUGIN_SCRIPTS_OVERRIDE="$SCRIPT_DIR/scripts"

# Build run_all.sh argument list
_args=""
if [ -n "$FILTER" ]; then
  _args="--filter $FILTER"
fi

# Run, tee to TAP log, capture exit code
set +e
if [ -n "$_args" ]; then
  sh "$RUN_ALL" $_args 2>&1 | tee "$TAP_LOG"
else
  sh "$RUN_ALL" 2>&1 | tee "$TAP_LOG"
fi
_rc=$?
set -e

printf '\n==> TAP results saved to: %s\n' "$TAP_LOG"

# Check for failures in the TAP output
_failures="$(grep -c '^not ok' "$TAP_LOG" 2>/dev/null || true)"
if [ "${_failures:-0}" -gt 0 ]; then
  printf '    FAILED: %d test(s) failed\n' "$_failures"
  exit 1
fi

if [ "$_rc" -ne 0 ]; then
  printf '    FAILED: run_all.sh exited with code %d\n' "$_rc"
  exit "$_rc"
fi

printf '    All tests passed\n'
exit 0
