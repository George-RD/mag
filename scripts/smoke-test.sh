#!/usr/bin/env bash
# smoke-test.sh — Integration smoke test: build, start daemon, verify MCP store+recall
#
# Usage:
#   bash scripts/smoke-test.sh
#
# Exits 0 on success, 1 on failure.
# Designed to run locally or in CI (no network required beyond the build).

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${REPO_DIR}/target/release/mag"

# ── Isolated environment ───────────────────────────────────────────────────────
WORK_DIR="$(mktemp -d)"
export HOME="${WORK_DIR}/home"
export MAG_DATA_ROOT="${WORK_DIR}/data"
mkdir -p "${HOME}" "${MAG_DATA_ROOT}"

DAEMON_PID=""
PASS=false

cleanup() {
    if [[ -n "${DAEMON_PID}" ]] && kill -0 "${DAEMON_PID}" 2>/dev/null; then
        kill "${DAEMON_PID}" 2>/dev/null || true
        wait "${DAEMON_PID}" 2>/dev/null || true
    fi
    rm -rf "${WORK_DIR}"
    if [[ "${PASS}" == true ]]; then
        echo ""
        echo "=== Smoke test PASSED ==="
    else
        echo ""
        echo "=== Smoke test FAILED ==="
        exit 1
    fi
}
trap cleanup EXIT

# ── Build ─────────────────────────────────────────────────────────────────────
echo "Building release binary..."
cd "${REPO_DIR}"
cargo build --release --bin mag 2>&1 | tail -5
echo "Build complete: ${BINARY}"

# ── Start daemon ─────────────────────────────────────────────────────────────
echo ""
echo "Starting mag serve..."
"${BINARY}" serve --port 0 >"${WORK_DIR}/daemon.log" 2>&1 &
DAEMON_PID=$!

# Wait for the daemon to write its port
PORT=""
for i in $(seq 1 20); do
    if ! kill -0 "${DAEMON_PID}" 2>/dev/null; then
        echo "Daemon exited unexpectedly. Log:" >&2
        cat "${WORK_DIR}/daemon.log" >&2
        exit 1
    fi
    PORT=$(grep -oE 'port [0-9]+|listening on.*:[0-9]+|bound to.*:[0-9]+' "${WORK_DIR}/daemon.log" 2>/dev/null \
           | grep -oE '[0-9]+$' | head -1 || true)
    if [[ -n "${PORT}" ]]; then
        break
    fi
    sleep 0.5
done

# Fallback: check if daemon is up on default port 7070
if [[ -z "${PORT}" ]]; then
    PORT=7070
    echo "Could not detect dynamic port — trying default ${PORT}"
fi

BASE_URL="http://127.0.0.1:${PORT}"
echo "Daemon on ${BASE_URL}"

# Wait for HTTP to be ready
for i in $(seq 1 20); do
    if curl -sf "${BASE_URL}/health" >/dev/null 2>&1; then
        break
    fi
    sleep 0.5
done

# ── MCP store call ────────────────────────────────────────────────────────────
echo ""
echo "Testing MCP store..."
STORE_PAYLOAD='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_store","arguments":{"content":"smoke test memory entry","importance":0.8}}}'
STORE_RESP=$(curl -sf -X POST "${BASE_URL}/mcp" \
    -H "Content-Type: application/json" \
    -d "${STORE_PAYLOAD}" 2>&1) || {
    echo "store call failed" >&2
    echo "Response: ${STORE_RESP}" >&2
    exit 1
}
echo "Store response: ${STORE_RESP}"

# Verify no error in response
if echo "${STORE_RESP}" | grep -q '"error"'; then
    echo "MCP store returned an error" >&2
    exit 1
fi

# ── MCP recall call ───────────────────────────────────────────────────────────
echo ""
echo "Testing MCP recall..."
RECALL_PAYLOAD='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"smoke test"}}}'
RECALL_RESP=$(curl -sf -X POST "${BASE_URL}/mcp" \
    -H "Content-Type: application/json" \
    -d "${RECALL_PAYLOAD}" 2>&1) || {
    echo "recall call failed" >&2
    echo "Response: ${RECALL_RESP}" >&2
    exit 1
}
echo "Recall response: ${RECALL_RESP}"

if echo "${RECALL_RESP}" | grep -q '"error"'; then
    echo "MCP recall returned an error" >&2
    exit 1
fi

# ── All checks passed ─────────────────────────────────────────────────────────
PASS=true
