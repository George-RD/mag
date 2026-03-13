#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROM_BIN="$ROOT_DIR/target/debug/mag"
PARITY_HOME="$(mktemp -d "${TMPDIR:-/tmp}/mag-parity-XXXXXX")"
trap 'rm -rf "$PARITY_HOME"' EXIT

echo "[parity] building MAG"
cargo build --manifest-path "$ROOT_DIR/Cargo.toml" >/dev/null

echo "[parity] running MAG ingest/search smoke"
ROM_INGEST_OUTPUT="$(HOME="$PARITY_HOME" USERPROFILE="$PARITY_HOME" "$ROM_BIN" ingest "parity-harness-sample")"
ROM_SEARCH_OUTPUT="$(HOME="$PARITY_HOME" USERPROFILE="$PARITY_HOME" "$ROM_BIN" search "parity-harness-sample" --limit 5)"
echo "mag ingest:      $ROM_INGEST_OUTPUT"
echo "mag search:      $ROM_SEARCH_OUTPUT"

if [[ -n "${OMEGA_MEMORY_CMD:-}" ]]; then
  echo "[parity] omega command detected; running smoke"
  set +e
  OMEGA_OUTPUT="$(bash -c "$OMEGA_MEMORY_CMD ingest 'parity-harness-sample'" 2>&1)"
  OMEGA_STATUS=$?
  set -e
  echo "omega status: $OMEGA_STATUS"
  echo "omega output: $OMEGA_OUTPUT"
else
  echo "[parity] OMEGA_MEMORY_CMD not set; skipped omega execution"
fi
