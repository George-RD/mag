"""Independent checks on the retained fresh-main CLI observation."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
OBSERVATION = ROOT / 'site/demos/observations/main-09c8634.json'


class DemoObservationTests(unittest.TestCase):
    def test_original_artifact_and_outcomes(self):
        data = OBSERVATION.read_bytes()
        self.assertEqual(hashlib.sha256(data).hexdigest(), '5e2208e74493ce5d017076bf86fe5335aaa7a98808fe803a916cad0dba570643')
        record = json.loads(data)
        self.assertEqual(record['declared_code_revision'], '09c863459cfb5943a58ecde306d303244df69b12')
        self.assertEqual(len(record['binary_sha256']), 64)
        self.assertEqual(len(record['model_sha256']), 2)
        self.assertEqual(len(record['attempts']), 7)
        self.assertTrue(all(item['exit_code'] == 0 for item in record['attempts']))
        rows = json.loads(record['attempts'][4]['stdout'])['results']
        decision = next(row for row in rows if row['event_type'] == 'decision')
        self.assertTrue(decision['content'].startswith('processed: Retries'))
        self.assertIn('entity:people:retries', decision['tags'])
        hits = json.loads(record['attempts'][5]['stdout'])['results']
        self.assertEqual(len(hits), 3)
        self.assertEqual(hits[0]['id'], decision['id'])
        self.assertEqual(hits[0]['score'], 1.0)
        self.assertEqual(json.loads(record['attempts'][6]['stdout']), {'results': []})

    def test_capture_refuses_existing_output_before_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'previous.json'
            path.write_text('retain original')
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/capture_demo_cli.py'),
                                     '--mag', 'missing-binary', '--models', 'missing-models',
                                     '--code-revision', 'declared', '--output', str(path)],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('output already exists', result.stderr)
            self.assertEqual(path.read_text(), 'retain original')


if __name__ == '__main__':
    unittest.main()
