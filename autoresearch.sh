#!/usr/bin/env bash
set -euo pipefail

# Build and run the Phase 2 extraction quality benchmark.
# Emits METRIC hit_rate for autoresearch consumption.

FEATURES="llm,substrate"
if cargo build --release --bin phase2_bench --features "$FEATURES" 2>&1; then
    ./target/release/phase2_bench "$@"
else
    echo "Build failed" >&2
    exit 1
fi
