"""Replay measured artifacts without treating historical results as quality gates."""
from collections import Counter
import hashlib
from pathlib import Path
import unittest
import zipfile

from benches.memory_intelligence import evaluate

ROOT = Path(__file__).resolve().parents[1]
BASELINES = ROOT / "benches/memory_intelligence/baselines"
ORIGINAL = "2026-09-09-lfm25-1.2b-q4km-cpu"
REJECTED = ORIGINAL + "-prompt-v2"
ORIGINAL_DIGEST = "01ed0eecc77d96b29efd5fcc596d3f23134d0806d6500c5a3148d3099f17d7dd"
REJECTED_DIGEST = "d1edb58a36ef676ce92ef29fd95b9eaf4cb53478f34a415a3723337dfe8f090c"


class BaselineEvidenceTests(unittest.TestCase):
    def replay(self, directory, digest):
        """Verify original ZIP bytes, rescore every attempt, and retain unknowns."""
        archive_path = BASELINES / directory / "evidence.zip"
        self.assertEqual(hashlib.sha256(archive_path.read_bytes()).hexdigest(), digest)
        with zipfile.ZipFile(archive_path) as archive:
            self.assertEqual(set(archive.namelist()), {
                "run.json", "scorecard.json", "pin.json", "cpu.json", "toolchain.txt",
            })
            run = evaluate.parse_json(archive.read("run.json").decode("utf-8"))
            recorded = evaluate.parse_json(archive.read("scorecard.json").decode("utf-8"))
            pin = evaluate.parse_json(archive.read("pin.json").decode("utf-8"))
        dataset = evaluate.load_json(ROOT / "benches/memory_intelligence/dataset.v1.json")
        self.assertEqual(evaluate.evaluate(dataset, run), recorded)
        self.assertEqual(run["model_profile"]["local_artifact"]["sha256"], pin["model"]["sha256"])
        self.assertEqual(run["model_profile"]["producer"]["verification"], "configured_not_authenticated")
        self.assertIsNone(run["model_profile"]["producer"]["checksums"])
        self.assertEqual(len(run["results"]), len(dataset["cases"]))
        self.assertEqual({result["case_id"] for result in run["results"]},
                         {case["id"] for case in dataset["cases"]})
        self.assertEqual(recorded["overall"]["grounded"]["true_positives"], 0)
        self.assertEqual(recorded["performance"]["latency_ms"]["samples"], 14)
        self.assertIsNone(recorded["performance"]["input_tokens"]["total"])
        self.assertIsNone(recorded["performance"]["output_tokens"]["total"])
        self.assertIsNone(recorded["performance"]["load_time_ms"])
        return run, recorded, pin

    def test_original_artifact_replays_without_changing_its_weak_result(self):
        """Keep the original three negative-control successes immutable."""
        run, recorded, _ = self.replay(ORIGINAL, ORIGINAL_DIGEST)
        self.assertEqual(run["code_revision"], "6a3c3d46b83c73a0a2ce12ba136c41b4b0811824")
        self.assertEqual(run["model_profile"]["producer"]["prompt_version"], 1)
        self.assertEqual(recorded["overall"]["task_success"]["succeeded"], 3)
        self.assertEqual(recorded["overall"]["schema_validity"]["valid"], 11)

    def test_rejected_prompt_v2_replays_without_dropping_regressions(self):
        """Valid JSON is not valid item schema or successful memory extraction."""
        run, recorded, _ = self.replay(REJECTED, REJECTED_DIGEST)
        self.assertEqual(run["code_revision"], "9eb7c6048e09d61c111b3e2d7760f1a27503de65")
        self.assertEqual(run["model_profile"]["producer"]["prompt_version"], 2)
        self.assertTrue(all("output" in result and "error" not in result
                            for result in run["results"]))
        self.assertEqual(recorded["overall"]["task_success"]["succeeded"], 2)
        self.assertEqual(recorded["overall"]["schema_validity"]["valid"], 4)
        self.assertEqual(recorded["overall"]["content"]["true_positives"], 0)
        self.assertEqual(recorded["overall"]["content"]["false_positives"], 3)
        self.assertEqual(recorded["overall"]["content"]["false_negatives"], 11)
        self.assertEqual({case["id"] for case in recorded["cases"] if case["success"]}, {
            "facts-proposal-is-not-a-fact", "questions-already-answered",
        })
        errors = Counter(case["error"] for case in recorded["cases"] if case["error"])
        self.assertEqual(errors, {
            "invalid output: item must be an object": 9,
            "invalid output: item has missing or unknown fields": 1,
        })
        outputs = {result["case_id"]: result["output"] for result in run["results"]}
        self.assertEqual(outputs["facts-explicit"], {"items": [None]})
        self.assertEqual(outputs["contradictions-explicit-supersession"], {"items": [None]})
        self.assertEqual(outputs["decisions-not-suggestions"], {"items": ["retain SQLite for Orbit"]})

    def test_rejected_experiment_source_delta_is_retained(self):
        """Keep the source and temporary runner that produced the rejected result."""
        patch = BASELINES / REJECTED / "experiment.patch"
        self.assertEqual(hashlib.sha256(patch.read_bytes()).hexdigest(),
                         "b7ff0fe568065c924c93d343146bf4aa0ec61bb138fef1a5e31923886c4cb0c9")

    def test_comparison_preserves_pins_without_claiming_identical_binaries(self):
        """Check shared inputs while retaining observed build differences."""
        original, _, original_pin = self.replay(ORIGINAL, ORIGINAL_DIGEST)
        rejected, _, rejected_pin = self.replay(REJECTED, REJECTED_DIGEST)
        self.assertEqual(original_pin, rejected_pin)
        self.assertEqual(original["dataset_sha256"], rejected["dataset_sha256"])
        for key in ("local_artifact", "runtime_build_claim"):
            self.assertEqual(original["model_profile"][key], rejected["model_profile"][key])
        # Ephemeral endpoint/alias and prompt version differ; decoding does not.
        excluded = {"base_url", "model", "prompt_version"}
        self.assertEqual(
            {k: v for k, v in original["model_profile"]["producer"].items() if k not in excluded},
            {k: v for k, v in rejected["model_profile"]["producer"].items() if k not in excluded},
        )
        settings = []
        for run in (original, rejected):
            args = run["measurement_context"]["local_baseline"]["server_arguments"]
            self.assertEqual(len(args) % 2, 0)
            settings.append({key: value for key, value in zip(args[::2], args[1::2])
                             if key not in {"--model", "--port", "--alias"}})
        self.assertEqual(*settings)
        for binary in ("mag_binary_sha256", "server_binary_sha256"):
            self.assertNotEqual(original["measurement_context"]["local_baseline"][binary],
                                rejected["measurement_context"]["local_baseline"][binary])


if __name__ == "__main__":
    unittest.main()
