#!/bin/sh
# tests/hooks/t04_error_capture.sh
# Verify the PostToolUse/error-capture hook fires when a build command fails.
#
# Strategy: ask Claude to run "cargo check" in an empty directory (no Cargo.toml).
# Cargo exits immediately with "error: could not find Cargo.toml" — no compilation
# or target/ writes needed, so the sandbox does not block it.
# "cargo check" matches error-capture.sh's fast-path; "error: " matches its output
# filter. cargo is required — test skips if not in PATH.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t04_error_capture"

# Prereq: cargo must be available
if ! command -v cargo >/dev/null 2>&1; then
  skip_test "cargo not in PATH"
fi

# Ask Claude to run cargo check in its working dir (no Cargo.toml present).
# Cargo will immediately output:
#   "error: could not find `Cargo.toml` in ..."
# which satisfies:
#   - fast-path filter: *"cargo check"*
#   - output filter:    *"error: "*
# No target/ dir is written, so no sandbox permission issues.
run_claude "Run: cargo check 2>&1" --max-turns 2

# 1. error_capture event must be in the JSONL log
assert_event_fired "hook.error_capture"

# 2. context.error_line must be non-empty
assert_jsonl_field_nonempty "hook.error_capture" ".context.error_line"

teardown_test
pass "error_capture hook fires and captures error_line"
