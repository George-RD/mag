"""Hermetic producer-boundary regressions; these are not model-quality evidence."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from benches.memory_intelligence import capture, evaluate

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = ROOT / "benches/memory_intelligence/capture.py"


def dataset():
    return {
        "schema_version": 1, "dataset_id": "private-dataset-identity",
        "cases": [
            {"id": "private-positive-case", "task": "facts",
             "instruction": "Extract the owner as owner=NAME.",
             "sources": [{"id": "m1", "text": "Iris owns the project."}],
             "expected": [{"value": "owner=Iris", "source_ids": ["m1"]}]},
            {"id": "private-negative-case", "task": "facts",
             "instruction": "Extract the owner as owner=NAME.",
             "sources": [{"id": "m2", "text": "The project has started."}],
             "expected": []},
        ],
    }


def metadata():
    return {
        "code_revision": "a" * 40,
        "model_profile": {"kind": "test-fixture", "model": None},
        "embedding_space_identity": None,
        "measurement_context": {"kind": "test-fixture", "hardware": "test process"},
    }


class RequestBoundaryTests(unittest.TestCase):
    def test_producer_receives_only_allowlisted_inputs(self):
        case = dataset()["cases"][0]
        request = capture.producer_request(case)
        self.assertEqual(set(request), {"schema_version", "task", "instruction", "sources"})
        self.assertEqual(request["schema_version"], 1)
        self.assertEqual(request["sources"], case["sources"])
        self.assertNotIn("private-", json.dumps(request))
        request["sources"][0]["text"] = "changed"
        self.assertEqual(case["sources"][0]["text"], "Iris owns the project.")

    def test_annotations_and_case_ids_cannot_change_the_request(self):
        original = dataset()["cases"][0]
        changed = copy.deepcopy(original)
        changed["id"] = "a label-bearing identifier"
        changed["expected"] = [{"value": "GOLD_ONLY_SENTINEL", "source_ids": ["m1"]}]
        self.assertEqual(capture.producer_request(original), capture.producer_request(changed))


@unittest.skipUnless(os.name == "posix", "capture is POSIX-only")
class CaptureTests(unittest.TestCase):
    def run_producer(self, code, **kwargs):
        return capture.capture_run(dataset(), metadata(), [sys.executable, "-S", "-c", code], **kwargs)

    def test_real_process_roundtrip_uses_sources_without_gold(self):
        code = """
import json, os, sys
request = json.load(sys.stdin)
assert set(request) == {'schema_version', 'task', 'instruction', 'sources'}
assert not os.listdir('.')
items = [{'value': 'owner=' + source['text'].split()[0], 'source_ids': [source['id']]}
         for source in request['sources'] if ' owns ' in source['text']]
print(json.dumps({'items': items}))
"""
        original_data, original_meta = dataset(), metadata()
        run = capture.capture_run(original_data, original_meta, [sys.executable, "-S", "-c", code])
        self.assertEqual(original_data, dataset())
        self.assertEqual(original_meta, metadata())
        self.assertEqual(run["model_profile"], original_meta["model_profile"])
        self.assertEqual(run["code_revision"], original_meta["code_revision"])
        report = evaluate.evaluate(original_data, run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 1)
        self.assertEqual(report["performance"]["latency_ms"]["samples"], 2)
        self.assertGreater(report["performance"]["latency_ms"]["p50"], 0)
        self.assertEqual(report["performance"]["input_tokens"], {"total": None, "samples": 0})
        self.assertEqual(report["performance"]["output_tokens"], {"total": None, "samples": 0})
        self.assertIsNone(report["performance"]["peak_ram_bytes"])
        self.assertIsNone(report["performance"]["load_time_ms"])
        self.assertIn("process startup", run["measurement_context"]["capture"]["latency_method"])

    def test_executable_symlinks_are_preserved_for_virtual_environments(self):
        with tempfile.TemporaryDirectory() as tmp:
            alias = Path(tmp) / "python-alias"
            alias.symlink_to(sys.executable)
            run = capture.capture_run(dataset(), metadata(), [str(alias), "-S", "-c", "print('{}')"])
            self.assertEqual(run["measurement_context"]["capture"]["producer_executable"], str(alias))

    def test_failed_case_does_not_hide_negative_control_or_stop_later_cases(self):
        run = self.run_producer("""
import json, sys
request = json.load(sys.stdin)
if request['sources'][0]['id'] == 'm1':
    sys.exit(7)
print('{"items":[]}')
""")
        self.assertEqual(len(run["results"]), 2)
        self.assertEqual(run["results"][0]["error"], "producer exited with code 7")
        self.assertIn("output", run["results"][1])
        report = evaluate.evaluate(dataset(), run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)
        self.assertEqual(report["overall"]["content"]["false_negatives"], 1)

    def test_nonzero_exit_cannot_turn_partial_valid_output_into_success(self):
        run = self.run_producer("import sys; print('{\"items\":[]}'); sys.exit(9)")
        self.assertTrue(all("error" in result and "output" not in result for result in run["results"]))
        self.assertEqual(evaluate.evaluate(dataset(), run)["overall"]["task_success"]["rate"], 0)

    def test_timeout_keeps_all_attempts_and_observed_latency(self):
        started = time.monotonic()
        run = self.run_producer("import time; time.sleep(30)", timeout=0.1)
        self.assertLess(time.monotonic() - started, 5)
        self.assertTrue(all(result["error"] == "producer timeout" for result in run["results"]))
        report = evaluate.evaluate(dataset(), run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 0)
        self.assertEqual(report["performance"]["latency_ms"]["samples"], 2)

    def test_closed_pipes_do_not_bypass_timeout(self):
        run = self.run_producer("import os, time; [os.close(fd) for fd in (0,1,2)]; time.sleep(30)", timeout=0.1)
        self.assertTrue(all(result["error"] == "producer timeout" for result in run["results"]))

    def test_stdout_and_stderr_have_independent_byte_bounds(self):
        for stream in ("stdout", "stderr"):
            with self.subTest(stream=stream):
                run = self.run_producer(f"import sys; sys.{stream}.write('x' * 10000)", max_bytes=512)
                self.assertTrue(all(stream + " exceeded byte limit" in r.get("error", "") for r in run["results"]))

    def test_oversized_requests_fail_before_a_process_starts(self):
        with mock.patch.object(capture.subprocess, "Popen") as start:
            run = self.run_producer("pass", max_bytes=1)
        start.assert_not_called()
        self.assertTrue(all("request exceeded" in r.get("error", "") for r in run["results"]))

    def test_full_pipes_do_not_deadlock_request_delivery(self):
        data = dataset()
        data["cases"][0]["sources"][0]["text"] = "x" * 200000
        code = "import sys; sys.stderr.write('x' * 200000); sys.stderr.flush(); sys.stdin.read(); print('{\"items\":[]}')"
        run = capture.capture_run(data, metadata(), [sys.executable, "-S", "-c", code], timeout=5, max_bytes=400000)
        self.assertTrue(all("output" in r for r in run["results"]))

    def test_invalid_json_utf8_and_nonfinite_values_become_failed_attempts(self):
        invalid = [b"not json", b'{"items":[],"items":[]}', b'{"x":NaN}', b'{"x":1e9999}',
                   b"\xff", b'{"x":"\\ud800"}', b"[" * 10000 + b"]" * 10000]
        for payload in invalid:
            with self.subTest(payload=payload[:30]):
                code = f"import sys; sys.stdout.buffer.write({payload!r})"
                run = self.run_producer(code)
                self.assertTrue(all(r.get("error") == "producer returned invalid UTF-8 JSON" for r in run["results"]))

    def test_wrong_json_shapes_are_preserved_for_the_scorer(self):
        for payload in ("null", "[]", '{"items":"bad"}', '{"items":[],"extra":1}'):
            with self.subTest(payload=payload):
                run = self.run_producer(f"print({payload!r})")
                self.assertEqual(run["results"][0]["output"], json.loads(payload))
                self.assertEqual(evaluate.evaluate(dataset(), run)["overall"]["schema_validity"]["rate"], 0)

    def test_stderr_is_not_echoed_or_recorded_as_model_output(self):
        run = self.run_producer("import sys; sys.stderr.write('PRIVATE_DIAGNOSTIC'); print('{\"items\":[]}')")
        self.assertNotIn("PRIVATE_DIAGNOSTIC", json.dumps(run))
        self.assertTrue(all("output" in r for r in run["results"]))

    def test_each_case_has_a_fresh_working_directory(self):
        code = "from pathlib import Path; assert not Path('state').exists(); Path('state').write_text('set'); print('{}')"
        run = self.run_producer(code)
        self.assertTrue(all("output" in r for r in run["results"]))

    def test_shell_metacharacters_are_literal_arguments(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "should-not-exist"
            argument = f"; touch {marker}"
            code = "import sys; assert sys.argv[1].startswith('; touch '); print('{}')"
            run = capture.capture_run(dataset(), metadata(), [sys.executable, "-S", "-c", code, argument])
            self.assertTrue(all("output" in r for r in run["results"]))
            self.assertFalse(marker.exists())

    def test_invalid_metadata_limits_and_command_do_not_start_processes(self):
        invalid_metadata = [{}, {**metadata(), "results": []}, {**metadata(), "schema_version": 1},
                            {**metadata(), "code_revision": "main"}, {**metadata(), "model_profile": {}},
                            {**metadata(), "measurement_context": {"capture": {}}}]
        with mock.patch.object(capture.subprocess, "Popen") as start:
            for meta in invalid_metadata:
                with self.subTest(meta=meta), self.assertRaises(ValueError):
                    capture.capture_run(dataset(), meta, [sys.executable])
            for timeout in (0, -1, True, float("nan"), float("inf"), 1e30):
                with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                    self.run_producer("pass", timeout=timeout)
            for limit in (0, -1, True, 1.2, 100000000):
                with self.subTest(limit=limit), self.assertRaises(ValueError):
                    self.run_producer("pass", max_bytes=limit)
            for command in ([], [""], ["missing-mag-producer-987654"], [sys.executable, "\0"], "echo foo"):
                with self.subTest(command=command), self.assertRaises(ValueError):
                    capture.capture_run(dataset(), metadata(), command)
        start.assert_not_called()

    def test_same_group_descendants_are_killed_on_timeout_and_parent_exit(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / "escaped"
            started = Path(tmp) / "started"
            child = f"import time; from pathlib import Path; time.sleep(2); Path({str(marker)!r}).write_text('escaped')"
            for parent_action in ("time.sleep(30)", "print('{}')"):
                with self.subTest(parent_action=parent_action):
                    started.write_text("")
                    code = ("import subprocess, sys, time; from pathlib import Path; "
                            f"p = subprocess.Popen([sys.executable, '-S', '-c', {child!r}], "
                            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); "
                            f"f = open({str(started)!r}, 'a'); f.write(str(p.pid) + '\\n'); f.close(); "
                            + parent_action)
                    run = self.run_producer(code, timeout=1 if "sleep" in parent_action else 5)
                    self.assertEqual(len(started.read_text().splitlines()), 2)
                    expected = "error" if "sleep" in parent_action else "output"
                    self.assertTrue(all(expected in r for r in run["results"]))
                    time.sleep(2.1)
                    self.assertFalse(marker.exists())


@unittest.skipUnless(os.name == "posix", "capture is POSIX-only")
class CaptureCliTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.data = self.root / "dataset.json"
        self.meta = self.root / "metadata.json"
        self.output = self.root / "run.json"
        self.data.write_text(json.dumps(dataset()), encoding="utf-8")
        self.meta.write_text(json.dumps(metadata()), encoding="utf-8")

    def invoke(self, *, output=None, code="print('{\"items\":[]}')"):
        return subprocess.run(
            [sys.executable, "-S", str(CAPTURE), str(self.data), "--metadata", str(self.meta),
             "--output", str(output or self.output), "--producer", sys.executable, "-S", "-c", code],
            capture_output=True, text=True, timeout=10,
        )

    def test_cli_writes_a_scoreable_run_without_mixing_output_streams(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")
        run = evaluate.load_json(self.output)
        self.assertEqual(evaluate.evaluate(dataset(), run)["overall"]["task_success"]["rate"], 0.5)
        self.assertEqual(len(list(self.root.iterdir())), 3)

    def test_failed_attempts_still_produce_an_artifact_with_exit_zero(self):
        result = self.invoke(code="raise SystemExit(4)")
        self.assertEqual(result.returncode, 0, result.stderr)
        run = evaluate.load_json(self.output)
        self.assertEqual(evaluate.evaluate(dataset(), run)["overall"]["task_success"]["rate"], 0)

    def test_input_aliases_are_rejected_before_running_a_producer(self):
        marker = self.root / "launched"
        code = f"from pathlib import Path; Path({str(marker)!r}).touch()"
        alias = self.root / "alias.json"
        alias.symlink_to(self.data)
        hardlink = self.root / "hardlink.json"
        os.link(self.meta, hardlink)
        for output in (self.data, self.meta, alias, hardlink):
            with self.subTest(output=output):
                result = self.invoke(output=output, code=code)
                self.assertEqual(result.returncode, 2)
                self.assertIn("must not overwrite", result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                self.assertFalse(marker.exists())
        self.assertEqual(evaluate.load_json(self.data), dataset())
        self.assertEqual(evaluate.load_json(self.meta), metadata())

    def test_invalid_input_keeps_existing_output_intact(self):
        self.output.write_text("previous run", encoding="utf-8")
        for payload in ('{"a":1,"a":2}', '[' * 2000 + ']' * 2000):
            self.meta.write_text(payload, encoding="utf-8")
            result = self.invoke()
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("Traceback", result.stderr)
            self.assertEqual(self.output.read_text(), "previous run")


if __name__ == "__main__":
    unittest.main()
