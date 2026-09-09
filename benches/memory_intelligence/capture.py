#!/usr/bin/env python3
"""Capture a trusted CLI producer's outputs; no model or memory semantics live here.

POSIX only. One process per case receives answer-blind JSON on stdin and emits
one JSON output on stdout. This is process supervision, not a security sandbox.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
from typing import Any

if __package__:
    from . import evaluate
else:
    import evaluate

METADATA_FIELDS = {
    "code_revision", "model_profile", "embedding_space_identity",
    "measurement_context", "load_time_ms", "peak_ram_bytes",
}


def producer_request(case: dict[str, Any]) -> dict[str, Any]:
    """Allowlist inputs; neither annotations nor label-bearing case IDs cross."""
    return {
        "schema_version": 1,
        "task": case["task"],
        "instruction": case["instruction"],
        "sources": copy.deepcopy(case["sources"]),
    }


class ProducerFailure(Exception):
    """An unsuccessful attempt that must remain in the scorecard denominator."""


def _stop_group(process: subprocess.Popen) -> None:
    # Also terminate descendants that outlived a successful parent. Producers
    # must not detach into another session; this is not hostile-code containment.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except PermissionError:
        # Darwin can report EPERM for a group containing only an unreaped
        # zombie. Reap our exited child, then retry the group once so live
        # descendants are not mistaken for successful cleanup. A live leader
        # or a second denial remains a real, visible cleanup failure.
        if process.poll() is None:
            raise
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait()


def _invoke(
    command: list[str], request: bytes, cwd: str, timeout: float, max_bytes: int,
) -> bytes:
    deadline = time.monotonic() + timeout
    process = subprocess.Popen(
        command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, cwd=cwd, start_new_session=True,
    )
    output = bytearray()
    counts = {"stdout": 0, "stderr": 0}
    pending = memoryview(request)
    group_stopped = False
    try:
        with selectors.DefaultSelector() as selector:
            for stream, name, event in (
                (process.stdin, "stdin", selectors.EVENT_WRITE),
                (process.stdout, "stdout", selectors.EVENT_READ),
                (process.stderr, "stderr", selectors.EVENT_READ),
            ):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, event, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ProducerFailure("producer timeout")
                # Descendants may retain inherited pipes after the parent exits.
                # Stop them before waiting for EOF, then drain buffered output.
                if not group_stopped and process.poll() is not None:
                    _stop_group(process)
                    group_stopped = True
                for key, _ in selector.select(min(remaining, 0.05)):
                    stream, name = key.fileobj, key.data
                    if name == "stdin":
                        try:
                            pending = pending[os.write(stream.fileno(), pending[:4096]):]
                        except BlockingIOError:
                            continue
                        except BrokenPipeError:
                            pending = pending[len(pending):]
                        if not pending:
                            selector.unregister(stream)
                            stream.close()
                    else:
                        try:
                            chunk = os.read(stream.fileno(), 65536)
                        except BlockingIOError:
                            continue
                        if not chunk:
                            selector.unregister(stream)
                            stream.close()
                            continue
                        counts[name] += len(chunk)
                        if counts[name] > max_bytes:
                            raise ProducerFailure(f"producer {name} exceeded byte limit")
                        if name == "stdout":
                            output.extend(chunk)
            try:
                process.wait(timeout=max(0, deadline - time.monotonic()))
            except subprocess.TimeoutExpired as exc:
                raise ProducerFailure("producer timeout") from exc
            if process.returncode != 0:
                raise ProducerFailure(f"producer exited with code {process.returncode}")
        return bytes(output)
    finally:
        try:
            if not group_stopped:
                _stop_group(process)
        finally:
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()


def capture_run(
    dataset: dict[str, Any], metadata: dict[str, Any], command: list[str],
    *, timeout: float = 60, max_bytes: int = 1024 * 1024,
) -> dict[str, Any]:
    """Capture every case, including failed attempts; validate before launching."""
    if os.name != "posix":
        raise ValueError("capture requires POSIX process groups (Linux or macOS)")
    if type(timeout) not in (int, float) or not math.isfinite(timeout) or not 0 < timeout <= 3600:
        raise ValueError("timeout must be finite and between 0 and 3600 seconds")
    if type(max_bytes) is not int or not 0 < max_bytes <= 16 * 1024 * 1024:
        raise ValueError("max_bytes must be an integer from 1 to 16777216")
    if not isinstance(command, list) or not command or any(
        not isinstance(arg, str) or "\0" in arg for arg in command
    ) or not command[0]:
        raise ValueError("producer must be a nonempty argument vector without NUL bytes")
    if not isinstance(metadata, dict) or metadata.keys() - METADATA_FIELDS:
        raise ValueError("metadata has unknown or reserved fields")
    cases = evaluate.validate_dataset(dataset)
    run = {
        **copy.deepcopy(metadata), "schema_version": 1,
        "dataset_sha256": evaluate.dataset_sha256(dataset), "results": [],
    }
    evaluate.evaluate(dataset, run)  # Reuse the scorer's authoritative validation.
    context = run["measurement_context"]
    if "capture" in context:
        raise ValueError("measurement_context.capture is reserved for the runner")
    executable = shutil.which(command[0])
    if executable is None:
        raise ValueError("producer executable was not found or is not executable")
    resolved_command = [str(Path(executable).absolute()), *command[1:]]
    context["capture"] = {
        "protocol": "mag-memory-intelligence-producer-v1",
        "producer_executable": resolved_command[0],
        "argv_sha256": hashlib.sha256(json.dumps(resolved_command).encode("utf-8")).hexdigest(),
        "timeout_seconds": timeout, "max_bytes_per_stream": max_bytes,
        "latency_method": "perf_counter_ns: per-case process startup, I/O, parsing and cleanup",
        "working_directory": "fresh temporary directory per case; not a sandbox",
        "environment": "inherited; not recorded",
        "resource_measurements": "tokens, model load time and peak RAM are not measured by this runner",
    }
    for case in cases.values():
        result: dict[str, Any] = {"case_id": case["id"]}
        started = time.perf_counter_ns()
        try:
            request = (json.dumps(producer_request(case), ensure_ascii=False) + "\n").encode("utf-8")
            if len(request) > max_bytes:
                raise ProducerFailure("producer request exceeded byte limit")
            with tempfile.TemporaryDirectory(prefix="mag-eval-") as cwd:
                raw = _invoke(resolved_command, request, cwd, timeout, max_bytes)
            try:
                output = evaluate.parse_json(raw.decode("utf-8"))
                # Reject escaped lone surrogates before artifact serialization.
                json.dumps(output, ensure_ascii=False, allow_nan=False).encode("utf-8")
            except (ValueError, TypeError, RecursionError) as exc:
                raise ProducerFailure("producer returned invalid UTF-8 JSON") from exc
            result["output"] = output  # Do not repair or selectively salvage malformed shapes.
        except ProducerFailure as exc:
            result["error"] = str(exc)
        except OSError as exc:
            result["error"] = f"producer I/O failure (errno {exc.errno})"
        finally:
            result["latency_ms"] = (time.perf_counter_ns() - started) / 1_000_000
        run["results"].append(result)
    evaluate.evaluate(dataset, run)
    return run


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=60)
    parser.add_argument("--max-bytes", type=int, default=1024 * 1024)
    parser.add_argument("--producer", nargs=argparse.REMAINDER, required=True)
    args = parser.parse_args()
    try:
        inputs = (args.dataset, args.metadata)
        evaluate.check_output_path(args.output, inputs)
        run = capture_run(
            evaluate.load_json(args.dataset), evaluate.load_json(args.metadata),
            args.producer, timeout=args.timeout_seconds, max_bytes=args.max_bytes,
        )
        text = json.dumps(run, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
        evaluate.write_report(args.output, text, inputs)
    except (OSError, ValueError, TypeError, RecursionError) as exc:
        parser.exit(2, f"error: {exc}\n")


if __name__ == "__main__":
    main()
