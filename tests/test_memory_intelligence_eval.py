"""Hermetic tests for the recorded-output evaluator; no model is invoked."""

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
EVALUATOR = ROOT / "benches/memory_intelligence/evaluate.py"
DATASET = ROOT / "benches/memory_intelligence/dataset.v1.json"
spec = importlib.util.spec_from_file_location("memory_intelligence_eval", EVALUATOR)
eval_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(eval_module)


def item(value="owner=Iris", sources=None):
    return {"value": value, "source_ids": ["m1"] if sources is None else sources}


def fixture():
    dataset = {
        "schema_version": 1,
        "dataset_id": "unit-test-only",
        "cases": [
            {
                "id": "fact-1",
                "task": "facts",
                "instruction": "Return owner=<name> for the current owner.",
                "sources": [
                    {"id": "m1", "text": "Iris owns Orbit."},
                    {"id": "m2", "text": "Neptune uses SQLite."},
                ],
                "expected": [item()],
            },
            {
                "id": "fact-2",
                "task": "facts",
                "instruction": "Return owner=<name> for the current owner.",
                "sources": [{"id": "m1", "text": "Noah owns Neptune."}],
                "expected": [item("owner=Noah")],
            },
        ],
    }
    run = {
        "schema_version": 1,
        "dataset_sha256": eval_module.dataset_sha256(dataset),
        "code_revision": "a" * 40,
        "model_profile": {"kind": "unit-test-fixture", "revision": "fixture-v1"},
        "embedding_space_identity": None,
        "measurement_context": {"source": "unit test; no model execution"},
        "results": [
            {"case_id": case["id"], "output": {"items": copy.deepcopy(case["expected"])}}
            for case in dataset["cases"]
        ],
    }
    return dataset, run


class ScoringTests(unittest.TestCase):
    def test_missing_case_is_counted_as_failure_and_false_negative(self):
        dataset, run = fixture()
        run["results"].pop()
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)
        self.assertEqual(report["overall"]["schema_validity"]["rate"], 0.5)
        self.assertEqual(report["overall"]["content"]["false_negatives"], 1)
        self.assertEqual(report["overall"]["content"]["recall"], 0.5)

    def test_reference_outputs_pass_without_claiming_measured_performance(self):
        dataset, run = fixture()
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 1.0)
        self.assertEqual(report["overall"]["grounded"]["recall"], 1.0)
        self.assertEqual(report["performance"]["latency_ms"], {"p50": None, "p95": None, "samples": 0})
        self.assertEqual(report["performance"]["input_tokens"], {"total": None, "samples": 0})
        self.assertIsNone(report["performance"]["load_time_ms"])
        self.assertIsNone(report["performance"]["peak_ram_bytes"])

    def test_wrong_or_invented_provenance_does_not_earn_grounded_credit(self):
        for source in ("m2", "invented"):
            with self.subTest(source=source):
                dataset, run = fixture()
                run["results"][0]["output"]["items"][0]["source_ids"] = [source]
                report = eval_module.evaluate(dataset, run)
                self.assertEqual(report["overall"]["content"]["recall"], 1.0)
                self.assertEqual(report["overall"]["grounded"]["recall"], 0.5)
                self.assertEqual(report["overall"]["schema_validity"]["rate"], 1.0)
                self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)
                expected = ["invented"] if source == "invented" else []
                self.assertEqual(report["cases"][0]["unknown_source_ids"], expected)

    def test_extra_predictions_reduce_precision_and_task_success(self):
        dataset, run = fixture()
        run["results"][0]["output"]["items"].append(item("owner=MadeUp"))
        report = eval_module.evaluate(dataset, run)
        self.assertAlmostEqual(report["overall"]["content"]["precision"], 2 / 3)
        self.assertEqual(report["overall"]["content"]["false_positives"], 1)
        self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)

    def test_multi_source_support_is_unordered_but_must_be_complete(self):
        dataset, run = fixture()
        dataset["cases"][0]["expected"][0]["source_ids"] = ["m1", "m2"]
        run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
        run["results"][0]["output"]["items"][0]["source_ids"] = ["m2", "m1"]
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["task_success"]["rate"], 1.0)
        run["results"][0]["output"]["items"][0]["source_ids"] = ["m1"]
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["grounded"]["recall"], 0.5)

    def test_invalid_output_is_a_failed_case_not_a_crashed_or_partial_score(self):
        invalid_outputs = [
            None, [], {}, {"items": "bad"}, {"items": [None]},
            {"items": [item(), item()]}, {"items": [item(sources=[])]},
            {"items": [item(sources=["m1", "m1"])]},
            {"items": [{"value": "owner=Iris"}]},
            {"items": [item(), {"value": 42, "source_ids": ["m1"]}]},
            {"items": [item()], "typo": True},
        ]
        for output in invalid_outputs:
            with self.subTest(output=output):
                dataset, run = fixture()
                run["results"][0]["output"] = output
                report = eval_module.evaluate(dataset, run)
                self.assertEqual(report["overall"]["schema_validity"]["rate"], 0.5)
                self.assertEqual(report["overall"]["content"]["recall"], 0.5)
                self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)

    def test_runtime_error_remains_in_denominator_and_keeps_observed_latency(self):
        dataset, run = fixture()
        run["results"][0] = {"case_id": "fact-1", "error": "timeout", "latency_ms": 1000}
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 0.5)
        self.assertEqual(report["performance"]["latency_ms"]["p95"], 1000)
        self.assertEqual(report["performance"]["attempted_cases"], 2)

    def test_negative_controls_require_valid_empty_output(self):
        dataset, run = fixture()
        dataset["cases"][0]["expected"] = []
        run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
        run["results"][0]["output"]["items"] = []
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["task_success"]["rate"], 1.0)
        run["results"][0]["output"] = None
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["task_success"]["rate"], 0.5)

    def test_empty_predictions_and_no_gold_have_undefined_precision_and_recall(self):
        dataset, run = fixture()
        for case, result in zip(dataset["cases"], run["results"]):
            case["expected"] = []
            result["output"]["items"] = []
        run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
        report = eval_module.evaluate(dataset, run)
        self.assertIsNone(report["overall"]["content"]["precision"])
        self.assertIsNone(report["overall"]["content"]["recall"])
        self.assertEqual(report["overall"]["task_success"]["rate"], 1.0)

    def test_per_task_scores_do_not_hide_a_failing_category(self):
        dataset, run = fixture()
        dataset["cases"][1]["task"] = "status"
        run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
        run["results"].pop()
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["by_task"]["facts"]["task_success"]["rate"], 1.0)
        self.assertEqual(report["by_task"]["status"]["task_success"]["rate"], 0.0)

    def test_profile_identity_and_revision_are_preserved_without_mutating_input(self):
        dataset, run = fixture()
        run["embedding_space_identity"] = "retriever-profile:v1:fixture"
        before = copy.deepcopy((dataset, run))
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["model_profile"], run["model_profile"])
        self.assertEqual(report["embedding_space_identity"], run["embedding_space_identity"])
        self.assertEqual(report["code_revision"], run["code_revision"])
        self.assertEqual((dataset, run), before)
        first = report["model_profile_sha256"]
        run["model_profile"]["revision"] = "fixture-v2"
        self.assertNotEqual(eval_module.evaluate(dataset, run)["model_profile_sha256"], first)

    def test_order_does_not_change_content_or_provenance_scores(self):
        dataset, run = fixture()
        run["results"].reverse()
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["task_success"]["rate"], 1.0)

    def test_labels_are_exact_not_casefolded_or_whitespace_normalized(self):
        dataset, run = fixture()
        run["results"][0]["output"]["items"][0]["value"] = "owner=iris"
        self.assertEqual(eval_module.evaluate(dataset, run)["overall"]["content"]["recall"], 0.5)


class ValidationTests(unittest.TestCase):
    def test_run_rejects_different_dataset_revision(self):
        dataset, run = fixture()
        dataset["cases"][0]["sources"][0]["text"] += " Updated."
        with self.assertRaisesRegex(ValueError, "dataset_sha256"):
            eval_module.evaluate(dataset, run)

    def test_duplicate_and_unknown_case_results_are_rejected(self):
        for result in ({"case_id": "unknown", "output": {"items": []}}, fixture()[1]["results"][0]):
            dataset, run = fixture()
            run["results"].append(result)
            with self.assertRaises(ValueError):
                eval_module.evaluate(dataset, run)

    def test_invalid_run_metadata_is_rejected(self):
        for field, value in [
            ("schema_version", 2), ("schema_version", True), ("code_revision", "main"),
            ("model_profile", {}), ("model_profile", None),
            ("embedding_space_identity", ""), ("measurement_context", {}),
            ("results", {}), ("typo", "ignored?"),
        ]:
            with self.subTest(field=field, value=value):
                dataset, run = fixture()
                run[field] = value
                with self.assertRaises(ValueError):
                    eval_module.evaluate(dataset, run)

    def test_ambiguous_result_envelopes_are_rejected(self):
        for change in ({"error": "failure"}, {"typo": 5}):
            dataset, run = fixture()
            run["results"][0].update(change)
            with self.assertRaises(ValueError):
                eval_module.evaluate(dataset, run)

    def test_invalid_dataset_annotations_are_rejected(self):
        mutations = [
            lambda d: d.update(schema_version=True),
            lambda d: d.update(cases=[]),
            lambda d: d["cases"].append(copy.deepcopy(d["cases"][0])),
            lambda d: d["cases"][0].update(task="unknown"),
            lambda d: d["cases"][0]["sources"].append(copy.deepcopy(d["cases"][0]["sources"][0])),
            lambda d: d["cases"][0]["expected"][0].update(source_ids=["unknown"]),
            lambda d: d["cases"][0]["expected"].append(item()),
            lambda d: d["cases"][0].update(instruction=" "),
            lambda d: d["cases"][0].update(typo=True),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                dataset, run = fixture()
                mutate(dataset)
                run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
                with self.assertRaises(ValueError):
                    eval_module.evaluate(dataset, run)

    def test_nonfinite_negative_boolean_or_fractional_measurements_are_rejected(self):
        for field in ("latency_ms", "input_tokens", "output_tokens"):
            bad_values = [-1, True, float("nan"), float("inf"), "10"]
            if field != "latency_ms":
                bad_values.append(1.5)
            for value in bad_values:
                with self.subTest(field=field, value=value):
                    dataset, run = fixture()
                    run["results"][0][field] = value
                    with self.assertRaises(ValueError):
                        eval_module.evaluate(dataset, run)
        for field, value in (("peak_ram_bytes", 2.5), ("load_time_ms", -1)):
            dataset, run = fixture()
            run[field] = value
            with self.assertRaises(ValueError):
                eval_module.evaluate(dataset, run)

    def test_canonical_dataset_digest_ignores_json_formatting_and_key_order(self):
        dataset, _ = fixture()
        reordered = dict(reversed(list(dataset.items())))
        self.assertEqual(eval_module.dataset_sha256(dataset), eval_module.dataset_sha256(reordered))
        self.assertEqual(eval_module.dataset_sha256(dataset), eval_module.dataset_sha256(json.loads(json.dumps(dataset, indent=4))))

    def test_json_reader_rejects_overflowing_numbers(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "overflow.json"
            path.write_text('{"value": 1e9999}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "nonfinite"):
                eval_module.load_json(path)

    def test_json_reader_rejects_duplicate_keys_and_nonfinite_constants(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.json"
            for text in ('{"a": 1, "a": 2}', '{"value": NaN}', '{"value": Infinity}'):
                path.write_text(text, encoding="utf-8")
                with self.assertRaises(ValueError):
                    eval_module.load_json(path)


class PerformanceTests(unittest.TestCase):
    def test_measured_values_keep_sample_counts_and_nearest_rank_percentiles(self):
        dataset, run = fixture()
        run["results"][0].update(latency_ms=10, input_tokens=0, output_tokens=3)
        run["results"][1].update(latency_ms=90, input_tokens=None)
        run.update(load_time_ms=123.5, peak_ram_bytes=4096)
        perf = eval_module.evaluate(dataset, run)["performance"]
        self.assertEqual(perf["latency_ms"], {"p50": 10, "p95": 90, "samples": 2})
        self.assertEqual(perf["input_tokens"], {"total": 0, "samples": 1})
        self.assertEqual(perf["output_tokens"], {"total": 3, "samples": 1})
        self.assertEqual(perf["load_time_ms"], 123.5)
        self.assertEqual(perf["peak_ram_bytes"], 4096)


class CliAndDatasetTests(unittest.TestCase):
    def test_checked_in_dataset_covers_all_tasks_with_negative_controls(self):
        dataset = eval_module.load_json(DATASET)
        eval_module.validate_dataset(dataset)
        self.assertEqual({case["task"] for case in dataset["cases"]}, eval_module.TASKS)
        self.assertGreaterEqual(sum(not case["expected"] for case in dataset["cases"]), 2)
        self.assertTrue(any("ليلى" in source["text"] for case in dataset["cases"] for source in case["sources"]))
        for case in dataset["cases"]:
            if case["task"] == "contradictions":
                self.assertIn("review_start_time", case["instruction"])
            if case["id"] == "facts-arabic-to-english":
                self.assertEqual(case["sources"][0]["text"], "ليلى تملك مشروع أطلس.")

    def test_all_checked_in_annotations_round_trip_as_reference_outputs(self):
        dataset = eval_module.load_json(DATASET)
        _, run = fixture()
        run["dataset_sha256"] = eval_module.dataset_sha256(dataset)
        run["results"] = [
            {"case_id": case["id"], "output": {"items": case["expected"]}}
            for case in dataset["cases"]
        ]
        report = eval_module.evaluate(dataset, run)
        self.assertEqual(report["overall"]["task_success"]["rate"], 1.0)
        self.assertEqual(len(report["by_task"]), 10)
        self.assertEqual(report["performance"]["latency_ms"]["samples"], 0)

    def test_cli_validation_reports_versioned_dataset_identity(self):
        result = subprocess.run([sys.executable, str(EVALUATOR), "validate", str(DATASET)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        result_json = json.loads(result.stdout)
        self.assertEqual(result_json["dataset_sha256"], eval_module.dataset_sha256(eval_module.load_json(DATASET)))

    def test_cli_scores_to_file_and_errors_without_a_traceback(self):
        dataset, run = fixture()
        with tempfile.TemporaryDirectory() as tmp:
            dataset_path, run_path, report_path = (Path(tmp) / name for name in ("dataset.json", "run.json", "report.json"))
            dataset_path.write_text(json.dumps(dataset), encoding="utf-8")
            run_path.write_text(json.dumps(run), encoding="utf-8")
            command = [sys.executable, str(EVALUATOR), "score", str(dataset_path), str(run_path), "--output", str(report_path)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(report_path.read_text())["overall"]["task_success"]["rate"], 1.0)
            self.assertEqual(result.stdout, "")
            run_path.write_text('{"broken": NaN}', encoding="utf-8")
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("Traceback", result.stderr)

    def test_cli_rejects_deeply_nested_json_without_a_traceback(self):
        dataset, _ = fixture()
        with tempfile.TemporaryDirectory() as tmp:
            dataset_path, nested_path = Path(tmp) / "dataset.json", Path(tmp) / "nested.json"
            dataset_path.write_text(json.dumps(dataset), encoding="utf-8")
            nested_path.write_text("[" * 10000 + "0" + "]" * 10000, encoding="utf-8")
            for arguments in (
                ["validate", str(nested_path)],
                ["score", str(dataset_path), str(nested_path)],
            ):
                with self.subTest(arguments=arguments):
                    result = subprocess.run(
                        [sys.executable, str(EVALUATOR), *arguments], capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertNotIn("Traceback", result.stderr)
                    self.assertEqual(result.stdout, "")

    def test_cli_refuses_to_overwrite_its_inputs(self):
        dataset, run = fixture()
        with tempfile.TemporaryDirectory() as tmp:
            dataset_path, run_path = Path(tmp) / "dataset.json", Path(tmp) / "run.json"
            dataset_path.write_text(json.dumps(dataset), encoding="utf-8")
            run_path.write_text(json.dumps(run), encoding="utf-8")
            before = dataset_path.read_bytes()
            result = subprocess.run([sys.executable, str(EVALUATOR), "score", str(dataset_path), str(run_path), "--output", str(dataset_path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(dataset_path.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
