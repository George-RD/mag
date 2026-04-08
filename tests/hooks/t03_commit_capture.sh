#!/bin/sh
# tests/hooks/t03_commit_capture.sh
# Verify the PostToolUse/commit-capture hook fires when Claude runs git commit.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t03_commit_capture"

# Create a temp git repo for Claude to commit into
TESTREPO="$TEST_TMPDIR/testrepo"
mkdir -p "$TESTREPO"

# Claude is instructed to initialise the repo and make a commit.
# The commit message contains a distinctive token so we can confirm capture.
run_claude "Run: cd $TESTREPO && git init && git config user.email 'test@example.com' && git config user.name 'Test User' && echo test > file.txt && git add . && git commit -m 'HOOKTEST_COMMIT_42'"

# 1. The commit-capture event must appear in the JSONL log
assert_event_fired "hook.commit_capture"

# 2. The captured commit message must contain our token
_line="$(get_event "hook.commit_capture" | head -1)"
_msg="$(printf '%s' "$_line" | jq -r '.context.commit_message' 2>/dev/null)"
case "$_msg" in
  *HOOKTEST_COMMIT*)
    : ;;
  *)
    fail "commit_message '$_msg' does not contain HOOKTEST_COMMIT"
    ;;
esac

teardown_test
pass "commit_capture hook fires and captures commit message"
