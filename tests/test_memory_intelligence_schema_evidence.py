"""Replay both measured schema modes without promoting a production profile."""
import hashlib
from pathlib import Path
import unittest
import zipfile

from benches.memory_intelligence import evaluate

ROOT = Path(__file__).resolve().parents[1]
BASELINES = ROOT / "benches/memory_intelligence/baselines"
ORIGINAL = "2026-09-09-lfm25-1.2b-q4km-cpu"


class SchemaEvidenceTests(unittest.TestCase):

    def test_schema_comparison_replays_both_arms_and_retains_semantic_failures(self):
        """This immutable observation is not a future-model quality threshold."""
        path = BASELINES / (ORIGINAL + "-schema") / "evidence.zip"
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(),
                         "436b4c1c23d653164f35f94e25b3ed83e6d6574119a51af5fffad7861064b88e")
        dataset = evaluate.load_json(ROOT / "benches/memory_intelligence/dataset.v1.json")
        runs, scores = {}, {}
        with zipfile.ZipFile(path) as archive:
            self.assertEqual(set(archive.namelist()), {
                "unconstrained.run.json", "unconstrained.scorecard.json",
                "json_schema.run.json", "json_schema.scorecard.json", "pin.json",
                "cpu.json", "toolchain.txt", "source-identity.txt",
                "measurement-workflow.yml", "measurement-only.patch",
            })
            pin = evaluate.parse_json(archive.read("pin.json").decode("utf-8"))
            for mode in ("unconstrained", "json_schema"):
                run = runs[mode] = evaluate.parse_json(archive.read(mode + ".run.json").decode("utf-8"))
                score = scores[mode] = evaluate.parse_json(archive.read(mode + ".scorecard.json").decode("utf-8"))
                self.assertEqual(evaluate.evaluate(dataset, run), score)
                self.assertEqual(run["code_revision"], "c1537d9b55b19bc9c3b673d18642e73be2d5da2d")
                self.assertEqual(run["model_profile"]["local_artifact"]["sha256"], pin["model"]["sha256"])
                self.assertEqual(run["model_profile"]["producer"]["output_mode"], mode)
                self.assertEqual(run["model_profile"]["producer"]["prompt_version"], 1)
                self.assertEqual([r["case_id"] for r in run["results"]], [c["id"] for c in dataset["cases"]])
                self.assertEqual(score["performance"]["latency_ms"]["samples"], 14)
                for kind in ("input_tokens", "output_tokens"):
                    self.assertIsNone(score["performance"][kind]["total"])
                self.assertIsNone(score["performance"]["load_time_ms"])
        plain, schema = runs["unconstrained"], runs["json_schema"]
        for key in ("local_artifact", "runtime_build_claim"):
            self.assertEqual(plain["model_profile"][key], schema["model_profile"][key])
        excluded = {"base_url", "model", "output_mode", "output_schema"}
        self.assertEqual(
            {k: v for k, v in plain["model_profile"]["producer"].items() if k not in excluded},
            {k: v for k, v in schema["model_profile"]["producer"].items() if k not in excluded},
        )
        settings = []
        for run in (plain, schema):
            args = run["measurement_context"]["local_baseline"]["server_arguments"]
            self.assertEqual(len(args) % 2, 0)
            settings.append({k: v for k, v in zip(args[::2], args[1::2])
                             if k not in {"--model", "--port", "--alias"}})
        self.assertEqual(*settings)
        for binary in ("mag_binary_sha256", "server_binary_sha256"):
            self.assertEqual(plain["measurement_context"]["local_baseline"][binary],
                             schema["measurement_context"]["local_baseline"][binary])
        for mode, valid, successes, positives in (("unconstrained", 11, 3, 0), ("json_schema", 13, 4, 1)):
            self.assertEqual(scores[mode]["overall"]["schema_validity"]["valid"], valid)
            self.assertEqual(scores[mode]["overall"]["task_success"]["succeeded"], successes)
            self.assertEqual(scores[mode]["overall"]["grounded"]["true_positives"], positives)
        self.assertEqual(scores["json_schema"]["overall"]["grounded"]["false_positives"], 3)
        self.assertEqual({case["id"] for case in scores["json_schema"]["cases"] if case["success"]}, {
            "facts-proposal-is-not-a-fact", "questions-already-answered",
            "contradictions-explicit-supersession", "facts-arabic-to-english",
        })
        self.assertEqual(sum("error" in r for r in plain["results"]), 2)
        self.assertEqual(sum("error" in r for r in schema["results"]), 0)


if __name__ == "__main__":
    unittest.main()
