#!/usr/bin/env bash
# smoke-test.sh — Integration smoke test: build mag, verify MCP JSON-RPC over stdio
#
# Usage:
#   bash scripts/smoke-test.sh
#
# mag serve is a stdio-only MCP server (JSON-RPC over stdin/stdout).
# There is no HTTP endpoint, no --port flag, no /health route.
# This script drives the MCP protocol from a Python subprocess client.
#
# Exits 0 on success, 1 on failure.
# Designed to run locally or in CI (no network required beyond the build).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
BINARY="${REPO_ROOT}/target/release/mag"

# ── Build ─────────────────────────────────────────────────────────────────────
if [[ ! -x "${BINARY}" ]]; then
    echo "Building mag (release)..."
    cargo build --release --manifest-path "${REPO_ROOT}/Cargo.toml"
    echo "Build complete: ${BINARY}"
else
    echo "Using existing binary: ${BINARY}"
fi

# ── Isolated environment ───────────────────────────────────────────────────────
TEMP_HOME="$(mktemp -d)"
PYCLIENT="${TEMP_HOME}/mcp_client.py"

cleanup() {
    rm -rf "${TEMP_HOME}"
}
trap cleanup EXIT

export MAG_DATA_ROOT="${TEMP_HOME}/.mag"
mkdir -p "${MAG_DATA_ROOT}"

# Write the Python MCP client to a file.
# It drives the full MCP handshake over stdin/stdout pipes, reading each
# response synchronously before sending the next message. This avoids the
# race condition where rmcp ignores messages sent before the initialized
# notification handshake completes.
cat > "${PYCLIENT}" << 'PYEOF'
#!/usr/bin/env python3
"""
Minimal MCP stdio client for smoke testing.
Spawns 'mag serve', drives the handshake, and validates responses.
"""
import json
import os
import subprocess
import sys

binary  = sys.argv[1]
data_root = sys.argv[2]
home_dir  = sys.argv[3]

env = os.environ.copy()
env["HOME"] = home_dir
env["MAG_DATA_ROOT"] = data_root

proc = subprocess.Popen(
    [binary, "serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    env=env,
)

def send(msg: dict) -> None:
    line = json.dumps(msg) + "\n"
    proc.stdin.write(line.encode())
    proc.stdin.flush()

def recv() -> dict:
    """Read lines until we get a valid JSON-RPC response (has 'id')."""
    while True:
        raw = proc.stdout.readline()
        if not raw:
            raise RuntimeError("server closed stdout unexpectedly")
        raw = raw.decode().strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError:
            # The instructions field may contain literal newlines — skip non-JSON lines
            continue
        if "id" in msg:
            return msg
        # Notifications (no id) are silently ignored

def assert_ok(resp: dict, label: str) -> dict:
    if "error" in resp:
        print(f"  FAIL [{label}]: JSON-RPC error: {resp['error']}", file=sys.stderr)
        sys.exit(1)
    if "result" not in resp:
        print(f"  FAIL [{label}]: no 'result' key; got: {list(resp.keys())}", file=sys.stderr)
        sys.exit(1)
    return resp["result"]

# ── Test 1: Initialize ────────────────────────────────────────────────────────
print("Test 1: initialize handshake")
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "smoke-test", "version": "0.1.0"},
}})
init_result = assert_ok(recv(), "initialize")
assert "serverInfo" in init_result or "capabilities" in init_result, \
    f"unexpected initialize result shape: {init_result}"
print("  PASS: initialize returned a valid result")

# Send the required initialized notification (no response expected)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

# ── Test 2: tools/list ────────────────────────────────────────────────────────
print("Test 2: tools/list")
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
list_result = assert_ok(recv(), "tools/list")
tools = list_result.get("tools", [])
names = {t["name"] for t in tools}
for required in ("memory_store", "memory_search"):
    if required not in names:
        print(f"  FAIL [tools/list]: '{required}' not in tool list", file=sys.stderr)
        sys.exit(1)
print(f"  PASS: tools/list returned {len(names)} tools (memory_store and memory_search present)")

# ── Test 3: memory_store ──────────────────────────────────────────────────────
print("Test 3: tools/call memory_store")
send({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
    "name": "memory_store",
    "arguments": {"content": "Smoke test memory entry", "importance": 0.8},
}})
store_result = assert_ok(recv(), "memory_store")
content = store_result.get("content", [])
if not content:
    print("  FAIL [memory_store]: no content in result", file=sys.stderr)
    sys.exit(1)
text = content[0].get("text", "")
try:
    payload = json.loads(text)
except json.JSONDecodeError:
    print(f"  FAIL [memory_store]: content text is not JSON: {text!r}", file=sys.stderr)
    sys.exit(1)
if "id" not in payload:
    print(f"  FAIL [memory_store]: no 'id' in payload: {payload}", file=sys.stderr)
    sys.exit(1)
print(f"  PASS: memory_store returned id {payload['id']}")

# Terminate cleanly
proc.stdin.close()
proc.wait(timeout=5)
PYEOF

echo ""
echo "Running MCP stdio smoke test..."
echo "  Binary:        ${BINARY}"
echo "  MAG_DATA_ROOT: ${MAG_DATA_ROOT}"
echo ""

# HOME is only set for the mag serve subprocess (inside the Python client).
# The cargo build above runs under the real HOME so it can access ~/.cargo.
python3 "${PYCLIENT}" "${BINARY}" "${MAG_DATA_ROOT}" "${TEMP_HOME}"

echo ""
echo "=== Smoke test PASSED ==="
