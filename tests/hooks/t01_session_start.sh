#!/bin/sh
# tests/hooks/t01_session_start.sh
# Verify the SessionStart hook fires and emits the expected JSONL fields.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t01_session_start"

run_claude "Echo HELLO"

# 1. Hook event must have been written to the JSONL log
assert_event_fired "hook.session_start"

# 2. session_id must be present and non-null
assert_jsonl_field_nonempty "hook.session_start" ".session_id"

# 3. hook.status must be "ok"
assert_jsonl_field "hook.session_start" ".hook.status" "ok"

# 4. schema version must be 0
assert_jsonl_field "hook.session_start" ".v" "0"

teardown_test
pass "SessionStart hook fires with required fields"
