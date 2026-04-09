#!/bin/sh
# tests/hooks/t06_subagent_stop.sh
# Verify the subagent_end hook if the model spawns a subagent.
# Because subagent spawning is model-dependent, this test skips if no event fires.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t06_subagent_stop"

# Ask Claude to use a subagent — allow extra turns for agent spawning
run_claude "Use a subagent to calculate 2+2" --max-turns 5

# Check whether the model actually spawned a subagent
_fired="$(get_event "hook.subagent_end")"

if [ -z "$_fired" ]; then
  skip_test "model did not spawn a subagent (hook.subagent_end not fired)"
fi

# If we reach here, a subagent was spawned — assert required fields
assert_jsonl_field_nonempty "hook.subagent_end" ".agent.id"
assert_jsonl_field "hook.subagent_end" ".hook.status" "ok"

teardown_test
pass "subagent_end hook fires with agent.id and status=ok"
