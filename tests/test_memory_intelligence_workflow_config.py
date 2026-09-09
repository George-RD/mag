"""Exercise the checked-in diagnostic step's shell failure boundary."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class WorkflowConfigTests(unittest.TestCase):
    def test_revision_lookup_failure_stops_before_diagnostic_invocation(self):
        workflow = (ROOT / ".github/workflows/memory-intelligence-eval.yml").read_text()
        step = workflow.split("      - name: Inspect actual CLI HTTP requests without running a model\n", 1)[1]
        step = step.split("      - uses:", 1)[0]
        commands = [line[10:] for line in step.splitlines() if line.startswith("          ")]
        self.assertIn("set -euo pipefail", commands)
        # Only these two commands are replaced; execute the actual step body.
        script = "git() { return 7; }\npython3() { printf 'diagnostic invoked'; }\n" + "\n".join(commands)
        with tempfile.TemporaryDirectory() as cwd:
            result = subprocess.run(["bash", "-c", script], cwd=cwd,
                                    env=dict(os.environ, RUNNER_TEMP=cwd),
                                    capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 7)
        self.assertNotIn("diagnostic invoked", result.stdout)


if __name__ == "__main__":
    unittest.main()
