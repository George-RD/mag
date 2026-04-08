#!/bin/sh
# tests/hooks/t05_prompt_gate.sh
# Verify the UserPromptSubmit/prompt-gate hook fires and classifies a store-intent prompt.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t05_prompt_gate"

# The word "remember" triggers the store-intent branch in prompt-gate.sh
run_claude "Please remember that the sky is blue"

# 1. prompt_gate event must appear in the JSONL log
assert_event_fired "hook.prompt_gate"

# 2. context.hint_type must equal "store"
assert_jsonl_field "hook.prompt_gate" ".context.hint_type" "store"

teardown_test
pass "prompt_gate hook fires with hint_type=store"
