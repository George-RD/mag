"""Template replay tests use a transport fixture, never a generation model."""
import base64
import copy
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zipfile
from http.server import BaseHTTPRequestHandler
from socketserver import TCPServer

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "benches/memory_intelligence/template_diagnostic.py"


def digest(raw):
    """Hash the observed bytes without decoding or normalizing them."""
    return hashlib.sha256(raw).hexdigest()


def request_report():
    """Build a source recorder fixture with deliberately noncanonical wire JSON."""
    raw = b'{ "model":"fixture", "messages":[{"role":"user","content":"source text"}], "temperature":0.1 }\n'
    return {"schema_version": 1, "kind": "http-request-diagnostic-no-generation",
            "code_revision": "a" * 40, "mag_binary_sha256": "b" * 64,
            "dataset_sha256": "c" * 64, "server_rendered_template": None,
            "requests": [{"case_id": "fixture-case", "http": {
                "method": "POST", "path": "/v1/chat/completions",
                "body_base64": base64.b64encode(raw).decode(),
                "body_sha256": digest(raw), "body": json.loads(raw)}}]}


@unittest.skipUnless(os.name == "posix", "POSIX process supervisor")
class TemplateDiagnosticTests(unittest.TestCase):
    def setUp(self):
        """Create a bounded local fixture without hostname resolution."""
        self.tmp = tempfile.TemporaryDirectory(prefix="mag-template-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "requests.json"
        self.source.write_text(json.dumps(request_report()), encoding="utf-8")
        self.context = {"label": "fixture, not a measured model", "revision": "d" * 40}
        self.calls = []
        self.mode = "ok"
        self.properties_fail = False
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def respond(self, body, status=200):
                self.send_response(status)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("X-Private-Fixture", "never-retain-header")
                self.end_headers()
                try:
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def do_GET(self):
                outer.calls.append((self.path, b""))
                self.respond(b'{"chat_template":"fixture template"}', 503 if outer.properties_fail else 200)

            def do_POST(self):
                body = self.rfile.read(int(self.headers["Content-Length"]))
                outer.calls.append((self.path, body))
                if outer.mode == "redirect":
                    self.send_response(307)
                    self.send_header("Location", outer.base + "/must-not-follow")
                    self.end_headers()
                    return
                if outer.mode == "drip":
                    self.send_response(200)
                    self.end_headers()
                    try:
                        for _ in range(25):
                            self.wfile.write(b" ")
                            self.wfile.flush()
                            time.sleep(0.05)
                    except (BrokenPipeError, ConnectionResetError):
                        pass
                    return
                replies = {
                    "invalid": (b'{not-json', 200),
                    "status": (b'{"error":"fixture failure"}', 500),
                    "shape": (b'{"prompt":null}', 200),
                    "oversized": (b'x' * 8192, 200),
                    "ok": (json.dumps({"prompt": "<start>\nArabic: \u0645\u0631\u062d\u0628\u0627\n<assistant>"}, ensure_ascii=False).encode(), 200),
                }
                self.respond(*replies[outer.mode])

        self.server = TCPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.01})
        self.thread.start()
        self.addCleanup(self.stop)

    def stop(self):
        """Close and join the fixture even after an assertion fails."""
        self.server.shutdown()
        self.thread.join()
        self.server.server_close()

    def run_diagnostic(self, **kwargs):
        """Load the implementation only when a test reaches its behavioral boundary."""
        module = importlib.import_module("benches.memory_intelligence.template_diagnostic")
        return module.inspect_templates(self.source, base_url=kwargs.pop("base_url", self.base),
                                        server_context=self.context, timeout=kwargs.pop("timeout", 2),
                                        max_bytes=kwargs.pop("max_bytes", 4096), **kwargs)

    def test_replays_exact_request_and_retains_observed_template_not_generation(self):
        """Wire bytes, provenance and Unicode survive independently of quality scores."""
        before = self.source.read_bytes()
        report = self.run_diagnostic()
        self.assertEqual(self.source.read_bytes(), before)
        self.assertEqual(report["kind"], "server-template-replay-no-generation")
        self.assertEqual(report["source_request_artifact"]["sha256"], digest(before))
        self.assertEqual(report["source_request_artifact"]["mag_binary_sha256"], "b" * 64)
        self.assertEqual(report["server_context"], self.context)
        self.assertIn("not authenticated", report["method"]["server_provenance"])
        self.assertEqual([path for path, _ in self.calls], ["/props", "/apply-template"])
        expected = base64.b64decode(request_report()["requests"][0]["http"]["body_base64"])
        self.assertEqual(self.calls[1][1], expected)
        attempt = report["templates"][0]
        self.assertEqual(attempt["request"], request_report()["requests"][0]["http"])
        self.assertEqual(attempt["prompt"], "<start>\nArabic: \u0645\u0631\u062d\u0628\u0627\n<assistant>")
        self.assertEqual(attempt["prompt_sha256"], digest(attempt["prompt"].encode()))
        response = attempt["response"]
        self.assertEqual(response["body_sha256"], digest(base64.b64decode(response["body_base64"])))
        self.assertIsNone(attempt["generation_prompt"])
        self.assertNotIn("error", attempt)
        self.assertNotIn("results", report)
        self.assertNotIn("model_profile", report)
        self.assertNotIn("never-retain-header", json.dumps(report))

    def test_failed_source_attempts_remain_without_replay(self):
        """A recorded failure is neither erased nor turned into a successful case."""
        source = request_report()
        source["requests"].insert(0, {"case_id": "failed", "http": None, "error": "timeout"})
        source["requests"].append({**copy.deepcopy(source["requests"][1]), "case_id": "failed-after-body", "error": "producer exited"})
        self.source.write_text(json.dumps(source))
        report = self.run_diagnostic()
        self.assertEqual(len(report["templates"]), 3)
        self.assertEqual(report["templates"][0]["source_error"], "timeout")
        self.assertEqual(report["templates"][2]["source_error"], "producer exited")
        self.assertTrue(all("error" in report["templates"][i] for i in (0, 2)))
        self.assertEqual([path for path, _ in self.calls], ["/props", "/apply-template"])

    def test_corrupt_source_integrity_is_rejected_before_network(self):
        """All source rows are validated before even querying server properties."""
        for field, value in (("body_sha256", "0" * 64), ("body_base64", "!"),
                             ("body", {"messages": []}), ("path", "/completion")):
            with self.subTest(field=field):
                source = request_report()
                source["requests"].append(copy.deepcopy(source["requests"][0]))
                source["requests"][1]["case_id"] = "corrupt"
                source["requests"][1]["http"][field] = value
                self.source.write_text(json.dumps(source))
                with self.assertRaises(ValueError):
                    self.run_diagnostic()
                self.assertEqual(self.calls, [])

    def test_rejects_wrong_report_kind_and_duplicate_case_ids(self):
        """Only the explicit recorded-request format can reach the server."""
        for change in (lambda r: r.update(kind="scorecard"),
                       lambda r: r.update(schema_version=True),
                       lambda r: r["requests"].append(copy.deepcopy(r["requests"][0]))):
            source = request_report()
            change(source)
            self.source.write_text(json.dumps(source))
            with self.assertRaises(ValueError):
                self.run_diagnostic()
        self.assertEqual(self.calls, [])

    def test_bad_server_responses_are_retained_without_retry(self):
        """Status, shape and parse failures retain the first actual response."""
        for mode in ("invalid", "status", "shape", "oversized", "redirect"):
            with self.subTest(mode=mode):
                self.mode = mode
                self.calls.clear()
                attempt = self.run_diagnostic()["templates"][0]
                self.assertIn("error", attempt)
                self.assertIsNone(attempt["prompt"])
                self.assertIsNone(attempt["generation_prompt"])
                self.assertIsNotNone(attempt["response"])
                self.assertEqual([path for path, _ in self.calls], ["/props", "/apply-template"])
                if mode == "oversized":
                    self.assertTrue(attempt["response"]["truncated"])
                    self.assertEqual(len(base64.b64decode(attempt["response"]["body_base64"])), 4096)

    def test_failed_properties_and_request_quota_remain_visible(self):
        """Server metadata failure does not hide cases or invent server identity."""
        self.properties_fail = True
        report = self.run_diagnostic(max_bytes=80)
        self.assertIn("server_properties_error", report)
        self.assertEqual(report["server_properties"]["status"], 503)
        self.assertIn("exceeded byte limit", report["templates"][0]["error"])
        self.assertEqual([path for path, _ in self.calls], ["/props"])

    def test_slow_drip_obeys_whole_response_deadline(self):
        """Per-read progress must not extend the bounded process deadline."""
        self.mode = "drip"
        started = time.monotonic()
        attempt = self.run_diagnostic(timeout=0.3)["templates"][0]
        self.assertLess(time.monotonic() - started, 1.5)
        self.assertIn("timeout", attempt["error"])
        self.assertEqual([path for path, _ in self.calls], ["/props", "/apply-template"])

    def test_only_literal_loopback_endpoints_and_bounded_limits(self):
        """Refuse credentials, redirection surfaces and unbounded budgets."""
        for url in ("http://example.com:80", "http://localhost:80", "https://127.0.0.1:80",
                    self.base + "/v1", self.base + "?x=1", "http://user@127.0.0.1:80",
                    "http://127.0.0.1:0", "http://127.0.0.1:65536"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                self.run_diagnostic(base_url=url)
        for timeout in (0, True, float("inf"), float("nan"), 601):
            with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                self.run_diagnostic(timeout=timeout)
        for maximum in (0, True, 0.5, 16777217):
            with self.subTest(maximum=maximum), self.assertRaises(ValueError):
                self.run_diagnostic(max_bytes=maximum)
        self.assertEqual(self.calls, [])

    def test_cli_preserves_artifacts_and_refuses_input_output_aliases(self):
        """The real script writes failures and does not overwrite its inputs."""
        context = self.root / "context.json"
        context.write_text(json.dumps(self.context))
        output = self.root / "output.json"
        command = [sys.executable, str(SCRIPT), str(self.source), "--base-url", self.base,
                   "--server-context", str(context), "--output", str(output)]
        self.mode = "invalid"
        result = subprocess.run(command, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("error", json.loads(output.read_text())["templates"][0])
        for path in (self.source, context):
            before = path.read_bytes()
            result = subprocess.run([*command[:-1], str(path)], capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(b"Traceback", result.stderr)
            self.assertEqual(path.read_bytes(), before)


class ArchivedTemplateObservationTests(unittest.TestCase):
    def test_archived_templates_preserve_original_requests_without_quality_claims(self):
        """Replay archived evidence integrity without starting a model or an HTTP server."""
        folder = ROOT / "benches/memory_intelligence/diagnostics"
        with zipfile.ZipFile(folder / "2026-09-09-http-request-v1/evidence.zip") as original:
            source_bytes = original.read("intelligence-request-diagnostic.json")
            source = json.loads(source_bytes)
        archive_path = folder / "2026-09-09-template-v1/evidence.zip"
        self.assertEqual(digest(archive_path.read_bytes()),
                         "77603b39db8538420c61355a6726f28ac84a0b38cd1c23a088492062240592dc")
        with zipfile.ZipFile(archive_path) as archive:
            self.assertEqual(archive.read("intelligence-request-diagnostic.json"), source_bytes)
            report = json.loads(archive.read("templates.json"))
            context = json.loads(archive.read("server-context.json"))
            pin = json.loads(archive.read("pin.json"))
        self.assertEqual(report["kind"], "server-template-replay-no-generation")
        self.assertEqual(report["source_request_artifact"]["sha256"], digest(source_bytes))
        self.assertEqual(report["server_context"], context)
        self.assertEqual(context["runtime_source_checkout"], pin["runtime"]["revision"])
        self.assertEqual(context["model_artifact"]["sha256"], pin["model"]["sha256"])
        self.assertEqual(context["model_artifact"]["verification"], "sha256_verified_private_copy")
        self.assertNotIn("server_properties_error", report)
        self.assertEqual(len(report["templates"]), len(source["requests"]))
        self.assertEqual(len(report["templates"]), 14)
        self.assertNotIn("results", report)
        self.assertNotIn("model_profile", report)
        for observed, request in zip(report["templates"], source["requests"]):
            with self.subTest(case_id=request["case_id"]):
                self.assertEqual(observed["case_id"], request["case_id"])
                self.assertEqual(observed["request"], request["http"])
                self.assertNotIn("error", observed)
                self.assertIsNone(observed["generation_prompt"])
                self.assertTrue(observed["prompt"].startswith("<|im_start|>system\n"))
                self.assertTrue(observed["prompt"].endswith("<|im_start|>assistant\n"))
                self.assertEqual(observed["prompt_sha256"], digest(observed["prompt"].encode()))
                response = observed["response"]
                raw = base64.b64decode(response["body_base64"], validate=True)
                self.assertEqual(response["body_sha256"], digest(raw))
                self.assertEqual(json.loads(raw), response["body"])
                self.assertEqual(response["status"], 200)
                self.assertFalse(response["truncated"])
                self.assertEqual(response["body"]["prompt"], observed["prompt"])
                previous = -1
                for message in request["http"]["body"]["messages"]:
                    position = observed["prompt"].find(message["content"])
                    self.assertGreater(position, previous)
                    previous = position


if __name__ == "__main__":
    unittest.main()
