"""
Minimal MCP stdio client for MAG.

Manages the mag serve subprocess and speaks JSON-RPC 2.0 over stdio.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional


class McpClient:
    """
    MCP client that wraps ``mag serve`` via stdio JSON-RPC.

    Usage::

        with McpClient() as client:
            client.call_tool("memory_store", {"content": "hello"})
    """

    _request_id = 0
    _lock: threading.Lock

    def __init__(
        self,
        mag_binary: Optional[str] = None,
        home_dir: Optional[str] = None,
        timeout_secs: float = 60.0,
    ):
        self.mag_binary = mag_binary or _find_mag_binary()
        self.home_dir = home_dir or os.path.expanduser("~/.mag")
        self.timeout_secs = timeout_secs
        self._proc: Optional[subprocess.Popen] = None
        self._lock = threading.Lock()

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def __enter__(self) -> McpClient:
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.stop()

    def start(self) -> None:
        """Start ``mag serve`` and perform the MCP handshake."""
        if self._proc is not None:
            return

        env = os.environ.copy()
        env["HOME"] = self.home_dir
        env["USERPROFILE"] = self.home_dir

        self._proc = subprocess.Popen(
            [self.mag_binary, "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=env,
        )

        # Initialize
        result = self._request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "mag-ama-bench", "version": "0.1.0"},
            },
        )
        if "error" in result:
            self.stop()
            raise RuntimeError(f"MCP initialize failed: {result['error']}")

        # Initialized notification
        self._notify("notifications/initialized")

        # Verify tools are available
        tools_result = self._request("tools/list", {})
        self._tools = [
            t["name"] for t in tools_result.get("result", {}).get("tools", [])
        ]

    def stop(self) -> None:
        """Terminate the subprocess gracefully."""
        if self._proc is None:
            return
        try:
            self._proc.terminate()
            self._proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait(timeout=5)
        except Exception:
            pass
        finally:
            self._proc = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def call_tool(self, name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
        """
        Call an MCP tool and return the parsed JSON result.

        The response ``content[0].text`` is automatically JSON-parsed
        (MAG returns tool results as JSON-encoded text blobs).
        """
        if self._proc is None:
            raise RuntimeError("MCP client not started")

        result = self._request("tools/call", {"name": name, "arguments": arguments})

        if "error" in result:
            raise McpToolError(
                f"Tool '{name}' failed: {result['error']}"
            )

        content = result.get("result", {}).get("content", [])
        if not content:
            return {}

        text = content[0].get("text", "")
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            # Some tools return plain text; wrap it
            return {"_raw": text}

    def store(self, content: str, **kwargs: Any) -> Dict[str, Any]:
        """Shorthand for ``memory_store``."""
        args: Dict[str, Any] = {"content": content}
        args.update(kwargs)
        return self.call_tool("memory_store", args)

    def store_batch(self, items: List[Dict[str, Any]]) -> Dict[str, Any]:
        """Shorthand for ``memory_store_batch``."""
        return self.call_tool("memory_store_batch", {"items": items})

    def search(
        self,
        query: str,
        mode: str = "text",
        limit: int = 10,
        advanced: bool = True,
        explain: bool = False,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """Shorthand for ``memory_search``."""
        args: Dict[str, Any] = {
            "query": query,
            "mode": mode,
            "limit": limit,
            "advanced": advanced,
            "explain": explain,
        }
        args.update(kwargs)
        return self.call_tool("memory_search", args)

    def search_with_scores(
        self,
        query: str,
        mode: str = "text",
        limit: int = 10,
        advanced: bool = True,
        **kwargs: Any,
    ) -> Dict[str, Any]:
        """
        Run memory_search with explain=true and annotate each result with its score.
        Returns the full response dict with 'results' containing score fields.
        """
        resp = self.search(
            query=query, mode=mode, limit=limit, advanced=advanced, explain=True, **kwargs
        )
        # MAG already returns the canonical blended `score` on each result when
        # advanced=True. Ensure every result has a `score` key for uniform access.
        for r in resp.get("results", []):
            if "score" not in r:
                meta = r.get("metadata", {})
                if "_text_overlap" in meta:
                    r["score"] = meta["_text_overlap"]
                else:
                    r["score"] = 0.0
        return resp

    # ------------------------------------------------------------------
    # Low-level I/O
    # ------------------------------------------------------------------

    def _request(self, method: str, params: Dict[str, Any]) -> Dict[str, Any]:
        with self._lock:
            self._request_id += 1
            req_id = self._request_id

        msg = {
            "jsonrpc": "2.0",
            "id": req_id,
            "method": method,
            "params": params,
        }
        self._send(msg)
        return self._recv_response(req_id)

    def _notify(self, method: str) -> None:
        msg = {"jsonrpc": "2.0", "method": method}
        self._send(msg)

    def _send(self, msg: Dict[str, Any]) -> None:
        if self._proc is None or self._proc.stdin is None:
            raise RuntimeError("MCP process not running")
        line = json.dumps(msg, ensure_ascii=False)
        self._proc.stdin.write(line + "\n")
        self._proc.stdin.flush()

    def _recv_response(self, expected_id: int) -> Dict[str, Any]:
        if self._proc is None or self._proc.stdout is None:
            raise RuntimeError("MCP process not running")

        deadline = time.time() + self.timeout_secs
        while time.time() < deadline:
            line = self._proc.stdout.readline()
            if not line:
                time.sleep(0.01)
                continue

            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue

            # Ignore notifications (no id field)
            if "id" not in msg:
                continue

            if msg.get("id") == expected_id:
                return msg

        raise TimeoutError(
            f"MCP response timeout (>{self.timeout_secs}s) waiting for id={expected_id}"
        )


class McpToolError(Exception):
    """Raised when an MCP tool call returns an error."""
    pass


def _find_mag_binary() -> str:
    """Locate the mag binary: prefer release build, then PATH."""
    # 1. Release build in project tree (when running from repo)
    project_release = Path(__file__).parent.parent.parent / "target" / "release" / "mag"
    if project_release.exists() and os.access(project_release, os.X_OK):
        return str(project_release)

    # 2. Development build
    project_debug = Path(__file__).parent.parent.parent / "target" / "debug" / "mag"
    if project_debug.exists() and os.access(project_debug, os.X_OK):
        return str(project_debug)

    # 3. Check for mag_memory package binary
    pkg_bin = Path(__file__).parent / "bin" / "mag"
    if pkg_bin.exists() and os.access(pkg_bin, os.X_OK):
        return str(pkg_bin)

    # 4. PATH
    for path_dir in os.environ.get("PATH", "").split(os.pathsep):
        candidate = Path(path_dir) / "mag"
        if candidate.exists() and os.access(candidate, os.X_OK):
            return str(candidate)

    raise RuntimeError(
        "mag binary not found. Build with: cargo build --release"
    )
