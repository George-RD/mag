#!/usr/bin/env python3
"""Run a trusted local llama.cpp baseline through MAG's existing CLI producer.

No download, extraction, or memory semantics live here. The caller supplies
trusted binaries and a model pin. File integrity is verified; execution is not
cryptographically attested. This is process supervision, not a security sandbox.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any
import urllib.error
import uuid

if __package__:
    from . import capture, evaluate
else:
    import capture
    import evaluate

MAX_BYTES = 1024 * 1024
# Cover the probe's five-second socket timeout plus interpreter startup, while
# _await_ready still caps each request to the remaining overall startup budget.
PROBE_TIMEOUT = 6.0
RSS_INTERVAL = 0.05


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _hex(value: Any, length: int, label: str) -> None:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{%d}" % length, value) is None:
        raise ValueError(f"{label} must be {length} lowercase hexadecimal characters")


def validate_pin(pin: Any) -> None:
    """Validate launch instructions, not a competing runtime model-profile schema."""
    evaluate._object(pin, {"schema_version", "model", "runtime"}, "baseline pin")
    evaluate._version(pin["schema_version"], "baseline pin")
    model = evaluate._object(pin["model"], {
        "repository", "revision", "filename", "sha256", "quantization", "licence",
    }, "pinned model")
    runtime = evaluate._object(pin["runtime"], {"repository", "revision"}, "pinned runtime")
    for key, value in model.items():
        evaluate._text(value, f"model {key}")
    evaluate._text(runtime["repository"], "runtime repository")
    _hex(model["sha256"], 64, "model sha256")
    _hex(model["revision"], 40, "model revision")
    _hex(runtime["revision"], 40, "runtime revision")


# urllib's socket timeout is per read, not a whole-response deadline. Keep the
# small standard-library health probe behind the capture supervisor so even
# drip-fed headers/body, redirects, and inherited pipes remain bounded.
_PROBE = r"""
import sys, urllib.request
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
with opener.open(sys.argv[1], timeout=5) as response:
    sys.stdout.buffer.write(response.read(65537))
"""


def _local_json(url: str, timeout: float) -> Any:
    try:
        with tempfile.TemporaryDirectory(prefix="mag-health-") as cwd:
            raw = capture._invoke([sys.executable, "-S", "-c", _PROBE, url], b"", cwd, timeout, 65536)
    except capture.ProducerFailure as exc:
        raise urllib.error.URLError("bounded local server probe failed") from exc
    return evaluate.parse_json(raw.decode("utf-8"))


def _await_ready(process: subprocess.Popen, base: str, alias: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while True:
        if process.poll() is not None:
            raise ValueError("local server exited before readiness")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError("local server readiness timeout")
        try:
            health = _local_json(base + "/health", min(remaining, PROBE_TIMEOUT))
            if not isinstance(health, dict) or health.get("status") != "ok":
                raise ValueError("local server returned invalid health status")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("local server readiness timeout")
            models = _local_json(base + "/v1/models", min(remaining, PROBE_TIMEOUT))
            if not isinstance(models, dict) or not isinstance(models.get("data"), list) or not any(
                isinstance(model, dict) and model.get("id") == alias for model in models["data"]
            ):
                raise ValueError("local server model alias did not match the owned launch")
            if process.poll() is not None:
                raise ValueError("local server exited before readiness")
            return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            time.sleep(min(0.05, max(0, deadline - time.monotonic())))


def read_peak_rss(pid: int) -> int | None:
    """Linux's observed server VmHWM, not host RAM, GPU RAM or a process-tree sum."""
    try:
        text = Path(f"/proc/{pid}/status").read_text(encoding="utf-8")
    except OSError:
        return None
    match = re.search(r"^VmHWM:\s+(\d+)\s+kB\s*$", text, re.MULTILINE)
    return int(match[1]) * 1024 if match else None


class _RssObserver:
    def __init__(self, pid: int):
        self.pid = pid
        self.peak: int | None = None
        self.samples = 0
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._observe, daemon=True)

    def _sample(self) -> None:
        value = read_peak_rss(self.pid)
        if value is not None:
            self.peak = max(self.peak or 0, value)
            self.samples += 1

    def _observe(self) -> None:
        self._sample()
        while not self.stop.wait(RSS_INTERVAL):
            self._sample()

    def finish(self) -> None:
        self.stop.set()
        self.thread.join()
        self._sample()


def run_baseline(
    dataset: dict[str, Any], pin: dict[str, Any], *, mag: Path, server: Path,
    model: Path, code_revision: str, startup_timeout: float = 120,
    case_timeout: int = 60, threads: int = 2,
) -> dict[str, Any]:
    """Verify bytes, own one local server, and reuse answer-blind capture/scoring."""
    if os.name != "posix":
        raise ValueError("local baseline requires POSIX process groups")
    validate_pin(pin)
    evaluate.validate_dataset(dataset)
    _hex(code_revision, 40, "code revision")
    if type(startup_timeout) not in (int, float) or not math.isfinite(startup_timeout) or not 0 < startup_timeout <= 600:
        raise ValueError("startup timeout must be finite and between 0 and 600 seconds")
    if type(case_timeout) is not int or not 1 <= case_timeout <= 600:
        raise ValueError("case timeout must be an integer from 1 to 600 seconds")
    if type(threads) is not int or not 1 <= threads <= 64:
        raise ValueError("threads must be an integer from 1 to 64")
    mag, server, model = (Path(path).absolute() for path in (mag, server, model))
    for path in (mag, server):
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ValueError("MAG and server must be executable files")
    if not model.is_file():
        raise ValueError("model must be a regular file")
    env_program = shutil.which("env")
    if env_program is None:
        raise ValueError("local baseline requires the POSIX env executable")
    binary_hashes = {"mag_binary_sha256": file_sha256(mag), "server_binary_sha256": file_sha256(server)}
    with tempfile.TemporaryDirectory(prefix="mag-local-baseline-") as temporary:
        root = Path(temporary)
        # Hash the exact private copy given to the server, not a mutable source path.
        staged = root / "model.gguf"
        shutil.copyfile(model, staged)
        if file_sha256(staged) != pin["model"]["sha256"]:
            raise ValueError("model checksum mismatch; no executable was started")
        staged.chmod(0o400)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        # A random alias detects accidental port reuse; it is not an auth secret.
        alias = "mag-eval-" + uuid.uuid4().hex
        base = f"http://127.0.0.1:{port}"
        producer = [env_program, "NO_PROXY=127.0.0.1,localhost", "no_proxy=127.0.0.1,localhost",
                    str(mag), "intelligence-produce", "--base-url", base + "/v1", "--model", alias,
                    "--timeout-seconds", str(case_timeout), "--max-tokens", "512"]
        with tempfile.TemporaryDirectory(prefix="mag-describe-") as cwd:
            try:
                raw = capture._invoke([*producer, "--describe"], b"", cwd, 10, MAX_BYTES)
                description = evaluate.parse_json(raw.decode("utf-8"))
                evaluate._object(description, {"model_profile", "embedding_space_identity"}, "producer description")
                profile = description["model_profile"]
                if not isinstance(profile, dict) or profile.get("role") != "generation" or profile.get("model") != alias or profile.get("base_url") != base + "/v1" or description["embedding_space_identity"] is not None:
                    raise ValueError("producer description did not match the configured generation command")
            except (capture.ProducerFailure, ValueError, TypeError, RecursionError) as exc:
                raise ValueError("could not obtain a matching bounded MAG producer description") from exc
        server_command = [str(server), "--model", str(staged), "--host", "127.0.0.1", "--port", str(port),
                          "--alias", alias, "--ctx-size", "4096", "--parallel", "1", "--threads", str(threads),
                          "--n-gpu-layers", "0", "--seed", "42", "--temp", "0.1", "--top-k", "50",
                          "--repeat-penalty", "1.05"]
        # Prevent inherited llama.cpp settings from silently changing the experiment.
        server_env = {key: value for key, value in os.environ.items() if not key.startswith("LLAMA_")}
        started = time.perf_counter_ns()
        process = subprocess.Popen(server_command, cwd=root, env=server_env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        observer = _RssObserver(process.pid)
        try:
            observer.thread.start()
            _await_ready(process, base, alias, startup_timeout)
            ready_ms = (time.perf_counter_ns() - started) / 1_000_000
            context = {
                "kind": "local-model-baseline; trusted caller-supplied binaries and pin",
                "hardware": {"os": platform.system(), "release": platform.release(),
                             "machine": platform.machine(), "logical_cpus": os.cpu_count()},
                "local_baseline": {
                    **binary_hashes, "server_ready_ms": ready_ms,
                    "readiness_method": "process start through healthy endpoint and unique model alias; excludes staging/download; includes initialization/warmup",
                    "rss_method": "sampled Linux /proc/<server-pid>/status VmHWM (kB * 1024), final sample before cleanup; excludes descendants, MAG, GPU and host totals",
                    "rss_interval_seconds": RSS_INTERVAL,
                    "unmeasured": "token usage and isolated model-load duration; unavailable RSS is null",
                    "server_arguments": server_command[1:],
                    "server_environment": "inherited except LLAMA_*; not recorded; producer loopback bypasses proxies",
                    "build_provenance": "source revisions are caller-declared; binary hashes are observed, not a source-to-binary attestation",
                    "trust_boundary": "trusted binaries/local host; SHA-256 verifies the staged model, not hostile-process execution",
                },
            }
            metadata = {
                "code_revision": code_revision,
                "model_profile": {
                    "producer": copy.deepcopy(profile),
                    "local_artifact": {**copy.deepcopy(pin["model"]), "size_bytes": staged.stat().st_size,
                                       "verification": "sha256_verified_private_copy"},
                    "runtime_build_claim": copy.deepcopy(pin["runtime"]),
                },
                "embedding_space_identity": None, "measurement_context": context,
                "load_time_ms": None,
            }
            run = capture.capture_run(dataset, metadata, producer, timeout=case_timeout + 5, max_bytes=MAX_BYTES)
        finally:
            try:
                if observer.thread.ident is not None:
                    observer.finish()
            finally:
                capture._stop_group(process)
        if file_sha256(staged) != pin["model"]["sha256"] or file_sha256(mag) != binary_hashes["mag_binary_sha256"] or file_sha256(server) != binary_hashes["server_binary_sha256"]:
            raise ValueError("baseline inputs changed during execution")
        run["peak_ram_bytes"] = observer.peak
        run["measurement_context"]["local_baseline"]["rss_samples"] = observer.samples
        evaluate.evaluate(dataset, run)
        return run


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    parser.add_argument("--pin", type=Path, required=True)
    parser.add_argument("--mag", type=Path, required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--code-revision", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--startup-timeout", type=float, default=120)
    parser.add_argument("--case-timeout", type=int, default=60)
    parser.add_argument("--threads", type=int, default=2)
    args = parser.parse_args()
    try:
        inputs = (args.dataset, args.pin, args.mag, args.server, args.model)
        evaluate.check_output_path(args.output, inputs)
        run = run_baseline(evaluate.load_json(args.dataset), evaluate.load_json(args.pin),
                           mag=args.mag, server=args.server, model=args.model, code_revision=args.code_revision,
                           startup_timeout=args.startup_timeout, case_timeout=args.case_timeout, threads=args.threads)
        evaluate.write_report(args.output, json.dumps(run, ensure_ascii=False, indent=2, allow_nan=False) + "\n", inputs)
    except (OSError, ValueError, TypeError, RecursionError) as exc:
        parser.exit(2, f"error: {exc}\n")


if __name__ == "__main__":
    main()
