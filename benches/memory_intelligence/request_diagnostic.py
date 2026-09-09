#!/usr/bin/env python3
"""Observe MAG's actual HTTP request bodies without running a model.

A bounded loopback recorder replaces the configured inference endpoint and
returns a labelled placeholder. Nothing is forwarded. This is request evidence,
not generation, a server-rendered template, a scorecard, or hostile-code isolation.
Use only trusted binaries and datasets whose source text may be written to disk.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import shutil
import socket
import tempfile
import threading
import time
from typing import Any

if __package__:
    from . import capture, evaluate
else:
    import capture
    import evaluate

PLACEHOLDER = "MAG_REQUEST_DIAGNOSTIC_NO_GENERATION"
MAX_HEADERS = 16384
PATH = "/v1/chat/completions"


class _Recorder:
    """One case, one retained body; absolute deadline includes headers and body."""

    def __init__(self, timeout: float, max_bytes: int):
        self.deadline = time.monotonic() + timeout
        self.max_bytes = max_bytes
        self.http: dict[str, Any] | None = None
        self.error: str | None = None
        self.connections = 0
        self.stop = threading.Event()
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(2)
        self.listener.setblocking(False)
        self.address = self.listener.getsockname()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join()
        self.listener.close()

    def _read(self, connection: socket.socket, maximum: int) -> bytes:
        while not self.stop.is_set():
            remaining = self.deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("HTTP request timeout")
            if select.select([connection], [], [], min(remaining, 0.05))[0]:
                chunk = connection.recv(maximum)
                if not chunk:
                    raise ValueError("incomplete HTTP request")
                return chunk
        raise ValueError("HTTP recorder stopped before request completion")

    def _body(self, connection: socket.socket) -> bytes:
        data = bytearray()
        while b"\r\n\r\n" not in data:
            if len(data) >= MAX_HEADERS:
                raise ValueError("HTTP headers exceeded byte limit")
            data.extend(self._read(connection, min(4096, MAX_HEADERS - len(data))))
        head, initial_body = bytes(data).split(b"\r\n\r\n", 1)
        body = bytearray(initial_body)
        lines = head.split(b"\r\n")
        if lines[0] not in (b"POST " + PATH.encode() + b" HTTP/1.1",
                            b"POST " + PATH.encode() + b" HTTP/1.0"):
            raise ValueError("unexpected HTTP method or path")
        lengths = []
        for line in lines[1:]:
            key, separator, value = line.partition(b":")
            if not separator:
                raise ValueError("malformed HTTP headers")
            if key.lower() == b"transfer-encoding":
                raise ValueError("chunked HTTP requests are not supported")
            if key.lower() == b"content-length":
                lengths.append(value.strip())
        if len(lengths) != 1 or not re.fullmatch(rb"[0-9]{1,9}", lengths[0]):
            raise ValueError("one bounded HTTP Content-Length is required")
        length = int(lengths[0])
        if length > self.max_bytes:
            raise ValueError("HTTP body exceeded byte limit")
        while len(body) < length:
            body.extend(self._read(connection, min(65536, length - len(body))))
        if len(body) != length:
            raise ValueError("unexpected bytes after HTTP body")
        return bytes(body)

    def _serve(self) -> None:
        response_body = json.dumps({"choices": [{"message": {"content": PLACEHOLDER}}]}).encode()
        response = (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                    + str(len(response_body)).encode() + b"\r\nConnection: close\r\n\r\n" + response_body)
        while not self.stop.is_set() and time.monotonic() < self.deadline:
            if not select.select([self.listener], [], [], 0.05)[0]:
                continue
            try:
                connection, _ = self.listener.accept()
            except BlockingIOError:
                continue
            self.connections += 1
            with connection:
                try:
                    body = self._body(connection)
                    if self.connections > 1:
                        self.error = "multiple HTTP requests for one case"
                    else:
                        self.http = {
                            "method": "POST", "path": PATH,
                            "body_base64": base64.b64encode(body).decode("ascii"),
                            "body_sha256": hashlib.sha256(body).hexdigest(),
                            "body": None,
                        }
                        try:
                            parsed = evaluate.parse_json(body.decode("utf-8"))
                            json.dumps(parsed, ensure_ascii=False, allow_nan=False).encode("utf-8")
                        except (ValueError, TypeError, RecursionError) as exc:
                            raise ValueError("HTTP body is not strict UTF-8 JSON") from exc
                        self.http["body"] = parsed
                    remaining = self.deadline - time.monotonic()
                    if remaining <= 0:
                        raise ValueError("HTTP response timeout")
                    connection.settimeout(remaining)
                    connection.sendall(response)
                except (OSError, ValueError) as exc:
                    self.error = self.error or (str(exc) if isinstance(exc, ValueError)
                                                else "HTTP recorder I/O failure")


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inspect_requests(
    dataset: dict[str, Any], *, mag: Path, code_revision: str,
    timeout: float = 10, max_bytes: int = 1024 * 1024,
) -> dict[str, Any]:
    """Keep every attempt; reuse the selected CLI and answer-blind input boundary."""
    if os.name != "posix":
        raise ValueError("request inspection requires POSIX process groups")
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 600:
        raise ValueError("timeout must be finite and between 0 and 600 seconds")
    if type(max_bytes) is not int or not 0 < max_bytes <= 16 * 1024 * 1024:
        raise ValueError("max_bytes must be an integer from 1 to 16777216")
    if not isinstance(code_revision, str) or re.fullmatch(r"[0-9a-f]{40}", code_revision) is None:
        raise ValueError("code revision must be a full lowercase Git commit SHA")
    cases = evaluate.validate_dataset(dataset)
    mag = Path(mag).absolute()
    if not mag.is_file() or not os.access(mag, os.X_OK):
        raise ValueError("MAG must be an executable file")
    env = shutil.which("env")
    if env is None:
        raise ValueError("request inspection requires the POSIX env executable")
    binary_sha = _file_sha256(mag)
    report = {
        "schema_version": 1, "kind": "http-request-diagnostic-no-generation",
        "code_revision": code_revision, "mag_binary_sha256": binary_sha,
        "dataset_sha256": evaluate.dataset_sha256(dataset),
        "server_rendered_template": None,
        "method": {
            "endpoint": "owned loopback recorder; no forwarding or model inference",
            "response": "fixed diagnostic placeholder, never scored or retained as model output",
            "request": "actual CLI HTTP body bytes; headers are not retained",
            "environment": "inherited except isolated HOME/USERPROFILE/MAG_DATA_ROOT and loopback proxy bypass",
            "limits": {"timeout_seconds": timeout, "body_bytes": max_bytes, "header_bytes": MAX_HEADERS},
            "provenance": "observed binary hash; code revision is caller-declared, not build attestation",
        },
        "requests": [],
    }
    for case in cases.values():
        attempt: dict[str, Any] = {"case_id": case["id"], "http": None}
        recorder = None
        try:
            request = json.dumps(capture.producer_request(case), ensure_ascii=False).encode("utf-8")
            if len(request) > max_bytes:
                raise capture.ProducerFailure("producer request exceeded byte limit")
            with tempfile.TemporaryDirectory(prefix="mag-request-diagnostic-") as cwd:
                with _Recorder(timeout, max_bytes) as recorder:
                    base = f"http://127.0.0.1:{recorder.address[1]}/v1"
                    command = [env, "NO_PROXY=127.0.0.1,localhost", "no_proxy=127.0.0.1,localhost",
                               f"HOME={cwd}", f"USERPROFILE={cwd}", f"MAG_DATA_ROOT={cwd}/unused",
                               str(mag), "intelligence-produce", "--base-url", base,
                               "--model", "mag-request-diagnostic", "--max-tokens", "512",
                               "--timeout-seconds", str(max(1, math.ceil(timeout)))]
                    output = capture._invoke(command, request, cwd, timeout, max_bytes)
                    if output.strip() != PLACEHOLDER.encode():
                        raise capture.ProducerFailure("producer did not return the diagnostic placeholder")
        except capture.ProducerFailure as exc:
            attempt["error"] = str(exc)
        except OSError:
            attempt["error"] = "request diagnostic I/O failure"
        finally:
            if recorder is not None:
                attempt["http"] = recorder.http
                if recorder.error:
                    attempt["error"] = recorder.error + ("; " + attempt["error"] if "error" in attempt else "")
            if attempt["http"] is None and "error" not in attempt:
                attempt["error"] = "no complete HTTP request recorded"
        report["requests"].append(attempt)
    if _file_sha256(mag) != binary_sha:
        raise ValueError("MAG binary changed during request inspection")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    parser.add_argument("--mag", type=Path, required=True)
    parser.add_argument("--code-revision", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=10)
    parser.add_argument("--max-bytes", type=int, default=1024 * 1024)
    args = parser.parse_args()
    try:
        inputs = (args.dataset, args.mag)
        evaluate.check_output_path(args.output, inputs)
        report = inspect_requests(evaluate.load_json(args.dataset), mag=args.mag,
                                  code_revision=args.code_revision, timeout=args.timeout_seconds,
                                  max_bytes=args.max_bytes)
        evaluate.write_report(args.output, json.dumps(report, ensure_ascii=False, indent=2,
                                                      allow_nan=False) + "\n", inputs)
    except (OSError, ValueError, TypeError, RecursionError) as exc:
        parser.exit(2, f"error: {exc}\n")


if __name__ == "__main__":
    main()
