"""Replay a historical measured artifact; this test does not assert model quality."""
import hashlib
from pathlib import Path
import unittest
import zipfile

from benches.memory_intelligence import evaluate

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / "benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu/evidence.zip"


class BaselineEvidenceTests(unittest.TestCase):
    def test_original_artifact_replays_without_changing_its_weak_result(self):
        self.assertEqual(
            hashlib.sha256(BASELINE.read_bytes()).hexdigest(),
            "01ed0eecc77d96b29efd5fcc596d3f23134d0806d6500c5a3148d3099f17d7dd",
        )
        with zipfile.ZipFile(BASELINE) as archive:
            self.assertEqual(set(archive.namelist()), {
                "run.json", "scorecard.json", "pin.json", "cpu.json", "toolchain.txt",
            })
            run = evaluate.parse_json(archive.read("run.json").decode("utf-8"))
            recorded = evaluate.parse_json(archive.read("scorecard.json").decode("utf-8"))
            pin = evaluate.parse_json(archive.read("pin.json").decode("utf-8"))
        dataset = evaluate.load_json(ROOT / "benches/memory_intelligence/dataset.v1.json")
        self.assertEqual(evaluate.evaluate(dataset, run), recorded)
        self.assertEqual(run["code_revision"], "6a3c3d46b83c73a0a2ce12ba136c41b4b0811824")
        self.assertEqual(run["model_profile"]["local_artifact"]["sha256"], pin["model"]["sha256"])
        self.assertEqual(run["model_profile"]["producer"]["verification"], "configured_not_authenticated")
        self.assertIsNone(run["model_profile"]["producer"]["checksums"])
        self.assertEqual(len(run["results"]), len(dataset["cases"]))
        self.assertEqual(recorded["overall"]["task_success"]["succeeded"], 3)
        self.assertEqual(recorded["overall"]["grounded"]["true_positives"], 0)
        self.assertEqual(recorded["performance"]["latency_ms"]["samples"], 14)
        self.assertIsNone(recorded["performance"]["output_tokens"]["total"])


if __name__ == "__main__":
    unittest.main()
