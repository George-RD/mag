#!/usr/bin/env bash
set -euo pipefail

# Build and run the substrate search pipeline benchmark.
# Emits METRIC lines for autoresearch consumption.

FEATURES="substrate"
if cargo build --release --bin substrate_bench --features "$FEATURES" 2>&1; then
    ./target/release/substrate_bench "$@"
else
    echo "Build failed" >&2
    exit 1
fi
