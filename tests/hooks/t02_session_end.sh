#!/bin/sh
# tests/hooks/t02_session_end.sh
# Verify the Stop/session_end hook fires and at least one memory is stored.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t02_session_end"

run_claude "Write: SESSION_END_MARKER_12345"

# 1. hook.session_end event must appear in the JSONL log
assert_event_fired "hook.session_end"

# 2. context.last_assistant_message must be non-empty
assert_jsonl_field_nonempty "hook.session_end" ".context.last_assistant_message"

# 3. At least one memory must have been stored during this session
assert_memory_stored 1

teardown_test
pass "SessionEnd hook fires, message captured, memory stored"
