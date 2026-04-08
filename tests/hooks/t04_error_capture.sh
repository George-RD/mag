#!/bin/sh
# tests/hooks/t04_error_capture.sh
# Verify the PostToolUse/error-capture hook fires when cargo check fails.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/helpers/common.sh"

setup_test "t04_error_capture"

# Prereq: cargo must be available
if ! command -v cargo >/dev/null 2>&1; then
  skip_test "cargo not in PATH"
fi

# Create a minimal Rust crate with a deliberate type error
BADCRATE="$TEST_TMPDIR/badcrate"
mkdir -p "$BADCRATE/src"

cat > "$BADCRATE/Cargo.toml" <<'TOML'
[package]
name = "badcrate"
version = "0.1.0"
edition = "2021"
TOML

# Deliberate type error: assign string to integer variable
cat > "$BADCRATE/src/main.rs" <<'RUST'
fn main() {
    let x: i32 = "this is not an integer";
    println!("{}", x);
}
RUST

# Ask Claude to run cargo check — the output will contain "error[E..."
run_claude "Run: cargo check --manifest-path $BADCRATE/Cargo.toml 2>&1"

# 1. error_capture event must be in the JSONL log
assert_event_fired "hook.error_capture"

# 2. context.error_line must be non-empty
assert_jsonl_field_nonempty "hook.error_capture" ".context.error_line"

teardown_test
pass "error_capture hook fires and captures error_line"
