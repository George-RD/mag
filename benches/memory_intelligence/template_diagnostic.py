#!/usr/bin/env python3
"""Replay captured MAG HTTP bodies through a trusted local server's template API.

The caller owns the running server. This client neither generates model output
nor authenticates the caller's server/build context. Source text is retained.
"""
from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
from typing import Any

if __package__:
    from . import capture, evaluate
else:
    import capture
    import evaluate

# Supervise the whole HTTP exchange, not just individual socket reads. Redirects
# and environment proxies are disabled. HTTP error bodies remain evidence; header
# values never enter the artifact. An over-limit body is an explicitly cut prefix.
_HTTP = r'''
import base64, json, sys, urllib.error, urllib.request
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
url, method, maximum = sys.argv[1], sys.argv[2], int(sys.argv[3])
data = sys.stdin.buffer.read() if method == "POST" else None
request = urllib.request.Request(url, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
try:
    response = opener.open(request, timeout=5)
except urllib.error.HTTPError as error:
    response = error
with response:
    raw = response.read(maximum + 1)
    print(json.dumps({"status": response.status,
                      "body_base64": base64.b64encode(raw[:maximum]).decode("ascii"),
                      "truncated": len(raw) > maximum}))
'''


def _sha256(raw: bytes) -> str:
    """Hash bytes without changing whitespace, encoding or template tokens."""
    return hashlib.sha256(raw).hexdigest()


def _source_requests(report: Any) -> list[tuple[dict[str, Any], bytes | None]]:
    """Validate the entire capture and every available body before contacting HTTP."""
    if not isinstance(report, dict):
        raise ValueError("source must be a request diagnostic object")
    evaluate._version(report.get("schema_version"), "source request diagnostic")
    if report.get("kind") != "http-request-diagnostic-no-generation":
        raise ValueError("source must be an HTTP request diagnostic, not a model run")
    for key, size in (("code_revision", 40), ("mag_binary_sha256", 64), ("dataset_sha256", 64)):
        value = report.get(key)
        if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{%d}" % size, value) is None:
            raise ValueError(f"invalid source {key}")
    requests = report.get("requests")
    if not isinstance(requests, list) or not requests:
        raise ValueError("source requests must be a nonempty array")
    rows, seen = [], set()
    for attempt in requests:
        if not isinstance(attempt, dict):
            raise ValueError("source attempt must be an object")
        case_id = attempt.get("case_id")
        evaluate._text(case_id, "source case ID")
        if case_id in seen:
            raise ValueError("duplicate source case ID")
        seen.add(case_id)
        if "error" in attempt:
            evaluate._text(attempt["error"], "source error")
        http = attempt.get("http")
        raw = None
        if http is None:
            if "error" not in attempt:
                raise ValueError("missing source HTTP body without an error")
        else:
            if not isinstance(http, dict) or http.get("method") != "POST" or http.get("path") != "/v1/chat/completions":
                raise ValueError("unexpected source HTTP method or path")
            try:
                raw = base64.b64decode(http["body_base64"], validate=True)
            except (ValueError, TypeError, KeyError) as exc:
                raise ValueError("invalid source body base64") from exc
            if _sha256(raw) != http.get("body_sha256"):
                raise ValueError("source body checksum mismatch")
            # Failed recorder attempts may retain invalid JSON with body=null.
            # Keep those failures, but never forward them as a successful request.
            if "error" not in attempt:
                parsed = evaluate.parse_json(raw.decode("utf-8"))
                if not isinstance(parsed, dict) or not isinstance(parsed.get("messages"), list):
                    raise ValueError("source body must contain chat messages")
                if evaluate._canonical(parsed) != evaluate._canonical(http.get("body")):
                    raise ValueError("source parsed body does not match recorded bytes")
        rows.append((attempt, raw))
    return rows


def _observe(url: str, method: str, body: bytes, timeout: float, maximum: int) -> tuple[dict[str, Any] | None, str | None]:
    """Retain a bounded response and safe failure reason from one HTTP attempt."""
    try:
        with tempfile.TemporaryDirectory(prefix="mag-template-http-") as cwd:
            result = capture._invoke([sys.executable, "-S", "-c", _HTTP, url, method, str(maximum)],
                                     body, cwd, timeout, maximum * 2 + 1024)
        response = evaluate.parse_json(result.decode("utf-8"))
    except capture.ProducerFailure as exc:
        return None, str(exc)
    except (OSError, ValueError, TypeError, RecursionError):
        return None, "template diagnostic HTTP exchange failed"
    raw = base64.b64decode(response["body_base64"], validate=True)
    response.update(body_sha256=_sha256(raw), body=None)
    if response["truncated"]:
        return response, "HTTP response exceeded byte limit; retained bytes are a prefix"
    try:
        parsed = evaluate.parse_json(raw.decode("utf-8"))
        evaluate._canonical(parsed)
        response["body"] = parsed
    except (ValueError, TypeError, RecursionError):
        return response, "HTTP response is not strict UTF-8 JSON"
    if response["status"] != 200:
        return response, f"HTTP response status {response['status']}"
    return response, None


def inspect_templates(request_artifact: Path, *, base_url: str, server_context: dict[str, Any],
                      timeout: float = 10, max_bytes: int = 1024 * 1024) -> dict[str, Any]:
    """Replay exact successful bodies once, preserving failed inputs and responses."""
    if os.name != "posix":
        raise ValueError("template inspection requires POSIX process groups")
    match = re.fullmatch(r"http://127\.0\.0\.1:([0-9]{1,5})", base_url) if isinstance(base_url, str) else None
    if match is None or not 1 <= int(match[1]) <= 65535:
        raise ValueError("base URL must be literal http://127.0.0.1:PORT without a path")
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 600:
        raise ValueError("timeout must be finite and between 0 and 600 seconds")
    if type(max_bytes) is not int or not 0 < max_bytes <= 16 * 1024 * 1024:
        raise ValueError("max_bytes must be an integer from 1 to 16777216")
    if not isinstance(server_context, dict):
        raise ValueError("caller-supplied server context must be an object")
    evaluate._canonical(server_context)
    source_path = Path(request_artifact)
    source_bytes = source_path.read_bytes()
    source = evaluate.parse_json(source_bytes.decode("utf-8"))
    requests = _source_requests(source)
    report = {
        "schema_version": 1, "kind": "server-template-replay-no-generation",
        "source_request_artifact": {"sha256": _sha256(source_bytes), **{
            key: source[key] for key in ("code_revision", "mag_binary_sha256", "dataset_sha256")}},
        "server_context": copy.deepcopy(server_context),
        "method": {
            "request": "exact recorded body bytes to /apply-template; no reconstruction or changed fields",
            "server_provenance": "caller-supplied context is preserved, not authenticated by this client",
            "scope": "template replay, not a captured generation prompt or a model-quality result",
            "endpoint": base_url, "headers": "not retained; redirects and environment proxies disabled",
            "limits": {"timeout_seconds_per_exchange": timeout, "body_bytes": max_bytes},
        },
        "templates": [],
    }
    properties, error = _observe(base_url + "/props", "GET", b"", timeout, max_bytes)
    report["server_properties"] = properties
    if error:
        report["server_properties_error"] = error
    for source_attempt, raw in requests:
        attempt = {"case_id": source_attempt["case_id"], "request": copy.deepcopy(source_attempt.get("http")),
                   "response": None, "prompt": None, "prompt_sha256": None, "generation_prompt": None}
        if "error" in source_attempt:
            attempt.update(source_error=source_attempt["error"], error="source request failed; not replayed")
        elif len(raw) > max_bytes:
            attempt["error"] = "source request exceeded byte limit; not replayed"
        else:
            response, error = _observe(base_url + "/apply-template", "POST", raw, timeout, max_bytes)
            attempt["response"] = response
            if not error:
                body = response["body"]
                if not isinstance(body, dict) or not isinstance(body.get("prompt"), str):
                    error = "template response must contain a string prompt"
                else:
                    attempt["prompt"] = body["prompt"]
                    attempt["prompt_sha256"] = _sha256(body["prompt"].encode("utf-8"))
            if error:
                attempt["error"] = error
        report["templates"].append(attempt)
    if _sha256(source_path.read_bytes()) != report["source_request_artifact"]["sha256"]:
        raise ValueError("source request artifact changed during inspection")
    return report


def main() -> None:
    """Write a separate diagnostic artifact; attempt failures remain visible inside it."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("requests", type=Path)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--server-context", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=10)
    parser.add_argument("--max-bytes", type=int, default=1024 * 1024)
    args = parser.parse_args()
    try:
        inputs = (args.requests, args.server_context)
        evaluate.check_output_path(args.output, inputs)
        report = inspect_templates(args.requests, base_url=args.base_url,
                                   server_context=evaluate.load_json(args.server_context),
                                   timeout=args.timeout_seconds, max_bytes=args.max_bytes)
        evaluate.write_report(args.output, json.dumps(report, ensure_ascii=False, indent=2,
                                                      allow_nan=False) + "\n", inputs)
    except (OSError, ValueError, TypeError, RecursionError) as exc:
        parser.exit(2, f"error: {exc}\n")


if __name__ == "__main__":
    main()
