"""Local baseline orchestration tests. All executables and model bytes are fixtures."""
import copy
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from benches.memory_intelligence import evaluate

ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "benches/memory_intelligence/local_baseline.py"


def fixture_dataset():
    return {"schema_version": 1, "dataset_id": "fixture-only", "cases": [
        {"id": "gold-owner-Iris", "task": "facts", "instruction": "Extract owner=NAME.",
         "sources": [{"id": "m1", "text": "Iris owns the project."}],
         "expected": [{"value": "owner=Iris", "source_ids": ["m1"]}]},
        {"id": "gold-negative", "task": "facts", "instruction": "Extract owner=NAME.",
         "sources": [{"id": "m2", "text": "There is no known owner."}], "expected": []},
    ]}


def runner():
    return importlib.import_module("benches.memory_intelligence.local_baseline")


SERVER = r'''
import http.server, json, os, sys
from pathlib import Path
args = sys.argv[1:]
def arg(key): return args[args.index(key) + 1]
Path(os.environ['TEST_SERVER_PID']).write_text(str(os.getpid()))
assert arg('--host') == '127.0.0.1'
assert not any(key.startswith('LLAMA_') for key in os.environ)
model = Path(arg('--model'))
assert model.read_bytes() == b'fixture model bytes'
assert not (model.stat().st_mode & 0o222)
mode = os.environ.get('TEST_SERVER_MODE', '')
if mode == 'exit': sys.exit(7)
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if mode == 'unready':
            self.send_response(503); self.end_headers(); return
        self.send_response(200); self.end_headers()
        if mode == 'drip':
            import time
            for _ in range(20):
                self.wfile.write(b' '); self.wfile.flush(); time.sleep(0.1)
            return
        payload = {'status': 'ok'} if self.path == '/health' else {
            'data': [{'id': 'wrong-alias' if mode == 'wrong' else arg('--alias')}]}
        self.wfile.write(json.dumps(payload).encode())
    def log_message(self, *args): pass
http.server.HTTPServer(('127.0.0.1', int(arg('--port'))), Handler).serve_forever()
'''

PRODUCER = r'''
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
def arg(key): return args[args.index(key) + 1]
assert args[0] == 'intelligence-produce'
assert os.environ['NO_PROXY'] == os.environ['no_proxy'] == '127.0.0.1,localhost'
if '--describe' in args:
    mode = os.environ.get('TEST_DESCRIBE_MODE', '')
    if mode == 'flood': print('x' * (2 * 1024 * 1024)); sys.exit()
    if mode == 'invalid': print('[]'); sys.exit()
    print(json.dumps({'model_profile': {
        'role': 'generation', 'model': 'wrong' if mode == 'wrong' else arg('--model'),
        'base_url': arg('--base-url'), 'verification': 'configured_not_authenticated',
        'revision': None, 'checksums': None, 'quantization': None, 'licence': None,
        'test_fixture': True,
    }, 'embedding_space_identity': None})); sys.exit()
request = json.load(sys.stdin)
assert set(request) == {'schema_version', 'task', 'instruction', 'sources'}
assert 'gold-' not in json.dumps(request)
assert not os.listdir('.')
with Path(os.environ['TEST_CALLS']).open('a') as f: f.write('case\n')
if os.environ.get('TEST_PRODUCER_FAIL'): sys.exit(9)
items = [{'value': 'owner=Iris', 'source_ids': ['m1']}] if request['sources'][0]['id'] == 'm1' else []
print(json.dumps({'items': items}))
'''


@unittest.skipUnless(os.name == "posix", "POSIX-only orchestration")
class LocalBaselineTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="mag-baseline-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.model = self.root / "model.gguf"
        self.model.write_bytes(b"fixture model bytes")
        self.mag = self.executable("mag-fixture", PRODUCER)
        self.server = self.executable("server-fixture", SERVER)
        self.pid = self.root / "server.pid"
        self.calls = self.root / "calls"
        self.environment = mock.patch.dict(os.environ, {
            "TEST_SERVER_PID": str(self.pid), "TEST_CALLS": str(self.calls),
            "LLAMA_ARG_MODEL": "must-not-be-used",
        })
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.pin = {"schema_version": 1, "model": {
            "repository": "fixture/not-a-model", "revision": "a" * 40,
            "filename": "model.gguf", "sha256": hashlib.sha256(self.model.read_bytes()).hexdigest(),
            "quantization": "fixture", "licence": "fixture-only",
        }, "runtime": {"repository": "fixture/not-a-runtime", "revision": "b" * 40}}

    def executable(self, name, code):
        path = self.root / name
        path.write_text(f"#!{sys.executable}\n" + code, encoding="utf-8")
        path.chmod(0o755)
        return path

    def run_baseline(self, **kwargs):
        return runner().run_baseline(
            fixture_dataset(), self.pin, mag=self.mag, server=self.server,
            model=self.model, code_revision="c" * 40,
            startup_timeout=kwargs.pop("startup_timeout", 3),
            case_timeout=kwargs.pop("case_timeout", 2), **kwargs,
        )

    def assert_server_stopped(self):
        pid = int(self.pid.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)

    def test_checksum_mismatch_fails_before_any_executable(self):
        self.pin["model"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "model checksum mismatch"):
            self.run_baseline()
        self.assertFalse(self.pid.exists())
        self.assertFalse(self.calls.exists())

    def test_invalid_pin_and_limits_fail_before_launch(self):
        for change in (lambda p: p.update(schema_version=True),
                       lambda p: p["model"].update(revision="main"),
                       lambda p: p["model"].update(sha256="z" * 64),
                       lambda p: p.update(extra=True)):
            with self.subTest(change=change):
                original = copy.deepcopy(self.pin)
                change(self.pin)
                with self.assertRaises(ValueError): self.run_baseline()
                self.pin = original
        for limit in (0, -1, True, float("inf"), float("nan"), 601):
            with self.subTest(limit=limit), self.assertRaises(ValueError):
                self.run_baseline(startup_timeout=limit)
        for limit in (0, True, 0.1, 601):
            with self.subTest(case_timeout=limit), self.assertRaises(ValueError):
                self.run_baseline(case_timeout=limit)
        self.assertFalse(self.pid.exists())

    def test_preserves_runtime_snapshot_and_records_local_file_evidence(self):
        original = copy.deepcopy(self.pin)
        run = self.run_baseline()
        self.assertEqual(self.pin, original)
        self.assertEqual(evaluate.evaluate(fixture_dataset(), run)["overall"]["task_success"]["rate"], 1)
        profile = run["model_profile"]
        self.assertEqual(profile["producer"]["verification"], "configured_not_authenticated")
        self.assertIsNone(profile["producer"]["revision"])
        self.assertTrue(profile["producer"]["test_fixture"])
        self.assertEqual(profile["local_artifact"]["sha256"], self.pin["model"]["sha256"])
        self.assertEqual(profile["local_artifact"]["verification"], "sha256_verified_private_copy")
        self.assertEqual(profile["local_artifact"]["size_bytes"], self.model.stat().st_size)
        self.assertIsNone(run["embedding_space_identity"])
        self.assertIsNone(run["load_time_ms"])
        context = run["measurement_context"]["local_baseline"]
        self.assertGreater(context["server_ready_ms"], 0)
        self.assertEqual(context["mag_binary_sha256"], hashlib.sha256(self.mag.read_bytes()).hexdigest())
        self.assertEqual(self.calls.read_text().splitlines(), ["case", "case"])
        self.assert_server_stopped()

    def test_failed_attempts_are_retained_without_retry(self):
        with mock.patch.dict(os.environ, {"TEST_PRODUCER_FAIL": "1"}):
            run = self.run_baseline()
        self.assertEqual(len(run["results"]), 2)
        self.assertTrue(all("error" in item for item in run["results"]))
        self.assertEqual(self.calls.read_text().splitlines(), ["case", "case"])
        self.assertEqual(evaluate.evaluate(fixture_dataset(), run)["overall"]["task_success"]["rate"], 0)
        self.assert_server_stopped()

    def test_wrong_server_identity_and_early_exit_are_rejected(self):
        for mode in ("wrong", "exit"):
            with self.subTest(mode=mode), mock.patch.dict(os.environ, {"TEST_SERVER_MODE": mode}):
                with self.assertRaises(ValueError): self.run_baseline()
                self.assert_server_stopped()
        self.assertFalse(self.calls.exists())

    def test_startup_deadline_cleans_up(self):
        started = time.monotonic()
        processes = []
        popen = subprocess.Popen
        def tracked(*args, **kwargs):
            process = popen(*args, **kwargs)
            processes.append(process)
            return process
        with mock.patch.dict(os.environ, {"TEST_SERVER_MODE": "unready"}), mock.patch.object(
            runner().subprocess, "Popen", side_effect=tracked,
        ):
            with self.assertRaisesRegex(ValueError, "readiness timeout"):
                self.run_baseline(startup_timeout=0.4)
        self.assertLess(time.monotonic() - started, 3)
        self.assertTrue(processes)
        for process in processes:
            with self.assertRaises(ProcessLookupError): os.kill(process.pid, 0)
        self.assertFalse(self.calls.exists())

    def test_dripping_health_body_cannot_extend_startup_deadline(self):
        started = time.monotonic()
        with mock.patch.dict(os.environ, {"TEST_SERVER_MODE": "drip"}):
            with self.assertRaisesRegex(ValueError, "readiness timeout"):
                self.run_baseline(startup_timeout=1)
        self.assertLess(time.monotonic() - started, 4)
        self.assert_server_stopped()

    def test_describe_is_bounded_and_invalid_profiles_are_rejected(self):
        for mode in ("wrong", "invalid", "flood"):
            with self.subTest(mode=mode), mock.patch.dict(os.environ, {"TEST_DESCRIBE_MODE": mode}):
                with self.assertRaises(ValueError): self.run_baseline()
        self.assertFalse(self.calls.exists())

    def test_unavailable_rss_is_null_not_zero(self):
        with mock.patch.object(runner(), "read_peak_rss", return_value=None):
            run = self.run_baseline()
        self.assertIsNone(run["peak_ram_bytes"])
        self.assertEqual(run["measurement_context"]["local_baseline"]["rss_samples"], 0)

    def test_rss_parser_respects_linux_units_and_missing_data(self):
        for text, expected in (("Name: server\nVmHWM:\t123 kB\n", 123 * 1024),
                               ("VmHWM: 3 MB\n", None), ("VmHWM: -1 kB\n", None), ("", None)):
            with self.subTest(text=text), mock.patch.object(Path, "read_text", return_value=text):
                self.assertEqual(runner().read_peak_rss(123), expected)
        with mock.patch.object(Path, "read_text", side_effect=FileNotFoundError):
            self.assertIsNone(runner().read_peak_rss(123))

    def test_cli_rejects_output_alias_before_launch(self):
        dataset = self.root / "dataset.json"
        dataset.write_text(json.dumps(fixture_dataset()))
        pin = self.root / "pin.json"
        pin.write_text(json.dumps(self.pin))
        output = self.root / "alias.gguf"
        os.link(self.model, output)
        result = subprocess.run([
            sys.executable, str(RUNNER), str(dataset), "--pin", str(pin),
            "--mag", str(self.mag), "--server", str(self.server), "--model", str(self.model),
            "--code-revision", "c" * 40, "--output", str(output),
        ], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertIn("input", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertEqual(self.model.read_bytes(), b"fixture model bytes")
        self.assertFalse(self.pid.exists())


if __name__ == "__main__":
    unittest.main()
