"""Wire observations must stay separate from generated-output evaluations."""
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

from benches.memory_intelligence import capture, evaluate, request_diagnostic as diagnostic

ROOT = Path(__file__).resolve().parents[1]
DATASET = {
    "schema_version": 1, "dataset_id": "annotation-canary-dataset",
    "cases": [{
        "id": "annotation-canary-case", "task": "facts",
        "instruction": "Extract owner=NAME.",
        "sources": [{"id": "s1", "text": "Iris owns café Orbit."}],
        "expected": [{"value": "annotation-canary-answer", "source_ids": ["s1"]}],
    }],
}

# A transport fixture, not a substitute model or Rust producer implementation.
FAKE = r'''#!/usr/bin/env python3
import json, os, sys, urllib.request
args = sys.argv
base = args[args.index('--base-url') + 1]
request = json.load(sys.stdin)
body = json.dumps({'model': args[args.index('--model') + 1],
                   'messages': [{'role': 'user', 'content': json.dumps(request)}]},
                  ensure_ascii=False).encode('utf-8')
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(urllib.request.Request(base + '/chat/completions', data=body,
                 headers={'Authorization': 'Bearer header-secret',
                          'Content-Type': 'application/json'})) as response:
    print(json.load(response)['choices'][0]['message']['content'], end='')
'''


class RequestDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.mag = self.root / "fake-mag"
        self.mag.write_text(FAKE, encoding="utf-8")
        self.mag.chmod(0o700)

    def inspect(self, dataset=None, **kwargs):
        return diagnostic.inspect_requests(
            DATASET if dataset is None else dataset, mag=self.mag,
            code_revision="a" * 40, **kwargs,
        )

    def test_records_exact_body_and_excludes_annotations_and_headers(self):
        report = self.inspect()
        self.assertEqual(report["kind"], "http-request-diagnostic-no-generation")
        self.assertNotIn("results", report)
        self.assertNotIn("model_profile", report)
        attempt = report["requests"][0]
        self.assertNotIn("error", attempt)
        body = base64.b64decode(attempt["http"]["body_base64"], validate=True)
        self.assertEqual(hashlib.sha256(body).hexdigest(), attempt["http"]["body_sha256"])
        wire = evaluate.parse_json(body.decode("utf-8"))
        request = json.loads(wire["messages"][0]["content"])
        self.assertEqual(request, capture.producer_request(DATASET["cases"][0]))
        self.assertNotIn("annotation-canary", body.decode("utf-8"))
        self.assertNotIn("header-secret", json.dumps(report))
        self.assertEqual(attempt["http"]["path"], "/v1/chat/completions")
        self.assertEqual(attempt["http"]["body"], wire)
        self.assertIsNone(report["server_rendered_template"])
        self.assertEqual(report["mag_binary_sha256"], hashlib.sha256(self.mag.read_bytes()).hexdigest())

    def test_retains_each_failed_case_without_synthetic_outputs(self):
        self.mag.write_text("#!/usr/bin/env python3\nraise SystemExit(7)\n")
        dataset = copy.deepcopy(DATASET)
        dataset["cases"].append({**dataset["cases"][0], "id": "second"})
        report = self.inspect(dataset)
        self.assertEqual(len(report["requests"]), 2)
        for attempt in report["requests"]:
            self.assertIn("error", attempt)
            self.assertNotIn("output", attempt)
            self.assertIsNone(attempt["http"])

    def test_keeps_captured_body_when_producer_exits_after_http(self):
        self.mag.write_text(FAKE + "\nraise SystemExit(7)\n")
        attempt = self.inspect()["requests"][0]
        self.assertIsNotNone(attempt["http"])
        self.assertIn("error", attempt)

    def test_rejects_duplicate_calls_without_replacing_first_request(self):
        self.mag.write_text(FAKE + r'''
with opener.open(urllib.request.Request(base + '/chat/completions', data=b'{}')) as r:
    r.read()
''')
        attempt = self.inspect()["requests"][0]
        self.assertIn("multiple", attempt["error"])
        self.assertIn("messages", attempt["http"]["body"])

    def test_bounds_producer_wall_time(self):
        self.mag.write_text("#!/usr/bin/env python3\nimport time\ntime.sleep(10)\n")
        start = time.monotonic()
        attempt = self.inspect(timeout=0.3)["requests"][0]
        self.assertIn("timeout", attempt["error"])
        self.assertLess(time.monotonic() - start, 3)

    def test_invalid_options_and_dataset_fail_before_execution(self):
        with mock.patch.object(capture, "_invoke") as invoke:
            for timeout in (0, -1, True, float("nan"), 601):
                with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                    self.inspect(timeout=timeout)
            for maximum in (0, True, 16 * 1024 * 1024 + 1):
                with self.subTest(maximum=maximum), self.assertRaises(ValueError):
                    self.inspect(max_bytes=maximum)
            with self.assertRaises(ValueError):
                self.inspect({})
            invoke.assert_not_called()

    def test_request_size_is_checked_before_invocation(self):
        with mock.patch.object(capture, "_invoke") as invoke:
            attempt = self.inspect(max_bytes=10)["requests"][0]
            self.assertIn("byte limit", attempt["error"])
            invoke.assert_not_called()

    def exchange(self, data, *, maximum=1024, timeout=1):
        with diagnostic._Recorder(timeout, maximum) as recorder:
            with socket.create_connection(recorder.address, timeout=2) as connection:
                try:
                    connection.sendall(data)
                    connection.shutdown(socket.SHUT_WR)
                    while connection.recv(4096):
                        pass
                except (BrokenPipeError, ConnectionResetError):
                    pass
        return recorder

    def test_malformed_and_oversized_http_are_visible_and_redacted(self):
        for headers, body in [
            (b"Content-Length: 9999", b""),
            (b"Content-Length: 2\r\nContent-Length: 2", b"{}"),
            (b"Transfer-Encoding: chunked", b"0\r\n\r\n"),
            (b"Content-Length: secret", b""),
            (b"Content-Length: 2", b"\xff\xff"),
            (b"Content-Length: 1", b"{"),
            (b"Content-Length: 3", b"NaN"),
            (b"Content-Length: 13", b'{"x":1,"x":2}'),
        ]:
            with self.subTest(headers=headers):
                recorder = self.exchange(b"POST /v1/chat/completions HTTP/1.1\r\n" + headers + b"\r\n\r\n" + body)
                self.assertIsNotNone(recorder.error)
                self.assertNotIn("secret", recorder.error)

    def test_stalled_headers_are_bounded_and_listener_is_closed(self):
        start = time.monotonic()
        with diagnostic._Recorder(0.25, 1024) as recorder:
            address = recorder.address
            with socket.create_connection(address, timeout=2) as connection:
                connection.sendall(b"POST /v1/chat/completions HTTP/1.1\r\n")
                time.sleep(0.35)
        self.assertLess(time.monotonic() - start, 2)
        self.assertIn("timeout", recorder.error)
        with self.assertRaises(OSError):
            socket.create_connection(address, timeout=0.1)

    def test_drip_fed_headers_cannot_extend_absolute_deadline(self):
        start = time.monotonic()
        with diagnostic._Recorder(0.25, 1024) as recorder:
            with socket.create_connection(recorder.address, timeout=2) as connection:
                def drip():
                    for _ in range(20):
                        try:
                            connection.sendall(b"x")
                        except OSError:
                            return
                        time.sleep(0.04)
                writer = threading.Thread(target=drip)
                writer.start()
                writer.join(timeout=2)
                self.assertFalse(writer.is_alive())
        self.assertLess(time.monotonic() - start, 2)
        self.assertIn("timeout", recorder.error)

    def test_rejects_unexpected_path_and_oversized_headers(self):
        recorder = self.exchange(b"POST /secret HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}")
        self.assertEqual(recorder.error, "unexpected HTTP method or path")
        recorder = self.exchange(b"X" * (diagnostic.MAX_HEADERS + 1))
        self.assertIn("headers exceeded", recorder.error)

    def test_changed_binary_cannot_be_published_as_a_stable_observation(self):
        self.mag.write_text(FAKE + "\nwith open(__file__, 'a') as f: f.write('# changed\\n')\n")
        with self.assertRaisesRegex(ValueError, "binary changed"):
            self.inspect()

    def test_output_input_alias_does_not_start_producer_or_overwrite(self):
        dataset_path = self.root / "dataset.json"
        original = json.dumps(DATASET)
        dataset_path.write_text(original)
        command = [sys.executable, str(ROOT / "benches/memory_intelligence/request_diagnostic.py"),
                   str(dataset_path), "--mag", str(self.mag), "--code-revision", "a" * 40,
                   "--output", str(dataset_path)]
        result = subprocess.run(command, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(dataset_path.read_text(), original)

    @unittest.skipUnless(os.environ.get("MAG_DIAGNOSTIC_BINARY"), "release MAG supplied by CI")
    def test_actual_selected_runtime_cli_wire_contract(self):
        dataset_path = ROOT / "benches/memory_intelligence/dataset.v1.json"
        dataset = evaluate.load_json(dataset_path)
        report = diagnostic.inspect_requests(dataset, mag=Path(os.environ["MAG_DIAGNOSTIC_BINARY"]),
                                             code_revision=os.environ["MAG_DIAGNOSTIC_REVISION"])
        output = os.environ.get("MAG_DIAGNOSTIC_OUTPUT")
        if output:
            evaluate.write_report(Path(output), json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                                  (dataset_path, Path(os.environ["MAG_DIAGNOSTIC_BINARY"])))
        self.assertEqual(len(report["requests"]), len(dataset["cases"]))
        for case, attempt in zip(dataset["cases"], report["requests"]):
            self.assertNotIn("error", attempt)
            body = attempt["http"]["body"]
            self.assertEqual(body["model"], "mag-request-diagnostic")
            self.assertEqual(body["max_tokens"], 512)
            self.assertAlmostEqual(body["temperature"], 0.1, places=6)
            self.assertNotIn("response_format", body)
            self.assertEqual([message["role"] for message in body["messages"]], ["system", "user"])
            self.assertEqual(json.loads(body["messages"][1]["content"]), capture.producer_request(case))


if __name__ == "__main__":
    unittest.main()
