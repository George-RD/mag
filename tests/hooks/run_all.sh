#!/bin/sh
# tests/hooks/run_all.sh
# Run all hook integration tests and print a TAP-compatible summary.
# Usage: run_all.sh [--filter <glob>]
#
# Exit codes:
#   0 — all tests passed (skips are OK)
#   1 — at least one test failed

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

FILTER=""
if [ "${1:-}" = "--filter" ]; then
  if [ -z "${2:-}" ]; then
    printf 'run_all: --filter requires a pattern argument\n' >&2
    exit 2
  fi
  FILTER="${2:-}"
  shift 2
fi

# Collect test scripts
ALL_TESTS=""
for _t in "$SCRIPT_DIR"/t*.sh; do
  [ -f "$_t" ] || continue
  if [ -n "$FILTER" ]; then
    case "$_t" in
      *$FILTER*) : ;;
      *) continue ;;
    esac
  fi
  ALL_TESTS="$ALL_TESTS $_t"
done

if [ -z "$ALL_TESTS" ]; then
  printf 'run_all: no tests matched (filter=%s)\n' "${FILTER:-(none)}" >&2
  exit 0
fi

# Count planned tests
_total=0
for _t in $ALL_TESTS; do _total=$(( _total + 1 )); done

printf '1..%d\n' "$_total"

_pass=0
_fail=0
_skip=0
_n=0

for _test in $ALL_TESTS; do
  _n=$(( _n + 1 ))
  _name="$(basename "$_test" .sh)"

  # Run the test script; capture its exit code
  _out="$(sh "$_test" 2>&1)"
  _rc=$?

  case "$_rc" in
    0)
      printf 'ok %d — %s\n' "$_n" "$_name"
      _pass=$(( _pass + 1 ))
      ;;
    77)
      # skip convention (automake / prove compatible)
      _skip_reason="$(printf '%s' "$_out" | grep '^SKIP' | head -1 | sed 's/^SKIP — [^:]*: //')"
      printf 'ok %d — %s # SKIP %s\n' "$_n" "$_name" "${_skip_reason:-skipped}"
      _skip=$(( _skip + 1 ))
      ;;
    *)
      _fail_reason="$(printf '%s' "$_out" | grep '^FAIL' | head -1 | sed 's/^FAIL — [^:]*: //')"
      printf 'not ok %d — %s\n' "$_n" "$_name"
      printf '  # FAILED: %s\n' "${_fail_reason:-exit code $_rc}"
      # Print full output for debugging
      printf '%s\n' "$_out" | sed 's/^/  # /'
      _fail=$(( _fail + 1 ))
      ;;
  esac
done

printf '\n# Results: %d passed, %d failed, %d skipped (total %d)\n' \
  "$_pass" "$_fail" "$_skip" "$_total"

if [ "$_fail" -gt 0 ]; then
  exit 1
fi
exit 0
