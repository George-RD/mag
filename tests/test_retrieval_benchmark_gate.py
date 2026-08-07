"""Contract tests for the repository-owned retrieval benchmark classifier."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "retrieval_benchmark_gate.py"
SPEC = importlib.util.spec_from_file_location("retrieval_benchmark_gate", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load classifier from {SCRIPT_PATH}")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class RetrievalBenchmarkGateTests(unittest.TestCase):
    def test_top_level_retrieval_scoring_and_reranking_files_are_governed(self) -> None:
        paths = [
            "src/memory_core/scoring.rs",
            "src/memory_core/scoring_strategy.rs",
            "src/memory_core/reranker.rs",
            "src/memory_core/retrieval_strategy.rs",
        ]

        self.assertEqual(GATE.benchmark_relevant_paths(paths), paths)

    def test_sqlite_retrieval_and_pipeline_paths_are_governed(self) -> None:
        paths = [
            "src/memory_core/storage/sqlite/search.rs",
            "src/memory_core/storage/sqlite/search_helpers.rs",
            "src/memory_core/storage/sqlite/advanced.rs",
            "src/memory_core/storage/sqlite/advanced/explain.rs",
            "src/memory_core/storage/sqlite/pipeline/scoring.rs",
            "src/memory_core/storage/sqlite/pipeline/decomp.rs",
        ]

        self.assertEqual(GATE.benchmark_relevant_paths(paths), paths)

    def test_unrelated_paths_do_not_require_the_benchmark(self) -> None:
        paths = [
            "README.md",
            "src/main.rs",
            "src/memory_core/domain.rs",
            "src/memory_core/storage/sqlite/session.rs",
            "tests/mcp_smoke.rs",
        ]

        self.assertEqual(GATE.benchmark_relevant_paths(paths), [])
        self.assertFalse(GATE.requires_benchmark(paths))

    def test_paths_are_normalized_and_deduplicated(self) -> None:
        paths = [
            "./src/memory_core/scoring.rs",
            "src\\memory_core\\scoring.rs",
            "src/memory_core/scoring.rs",
        ]

        self.assertEqual(
            GATE.benchmark_relevant_paths(paths),
            ["src/memory_core/scoring.rs"],
        )

    def test_cli_paths_mode_emits_machine_readable_boolean(self) -> None:
        relevant = subprocess.run(
            [
                sys.executable,
                str(SCRIPT_PATH),
                "--paths",
                "src/memory_core/reranker.rs",
            ],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        unrelated = subprocess.run(
            [sys.executable, str(SCRIPT_PATH), "--paths", "README.md"],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        )

        self.assertEqual(relevant.stdout.strip(), "true")
        self.assertEqual(unrelated.stdout.strip(), "false")


if __name__ == "__main__":
    unittest.main()
