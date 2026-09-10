"""Fail-closed static checks for the published walkthrough files."""
from pathlib import Path
import shutil
import tempfile
import unittest

from scripts.validate_site import validate_site

ROOT = Path(__file__).resolve().parents[1]


class SiteValidationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.site = Path(self.directory.name) / 'site'
        shutil.copytree(ROOT / 'site', self.site)

    def test_current_site(self):
        self.assertEqual(validate_site(self.site), [])

    def test_missing_demo(self):
        (self.site / 'demos/abstention.html').unlink()
        self.assertTrue(any('abstention.html' in item for item in validate_site(self.site)))

    def test_missing_shared_script(self):
        (self.site / 'assets/demo-player.js').unlink()
        self.assertTrue(any('demo-player.js' in item for item in validate_site(self.site)))

    def test_broken_link(self):
        path = self.site / 'demos/index.html'
        path.write_text(path.read_text() + '<a href="missing.html">Missing</a>')
        self.assertTrue(any('missing.html' in item for item in validate_site(self.site)))

    def test_broken_fragment(self):
        path = self.site / 'demos/index.html'
        path.write_text(path.read_text() + '<a href="../index.html#missing">Missing</a>')
        self.assertTrue(any('#missing' in item for item in validate_site(self.site)))

    def test_missing_title(self):
        path = self.site / 'demos/index.html'
        path.write_text(path.read_text().replace('<title>', '<not-title>').replace('</title>', '</not-title>'))
        self.assertTrue(any('metadata' in item for item in validate_site(self.site)))

    def test_case_collision(self):
        (self.site / 'INDEX.html').write_text('duplicate')
        self.assertTrue(any('case-insensitive collision' in item for item in validate_site(self.site)))

    def test_escape_outside_site(self):
        path = self.site / 'demos/index.html'
        path.write_text(path.read_text() + '<a href="../../private.txt">Outside</a>')
        self.assertTrue(any('outside site' in item for item in validate_site(self.site)))


if __name__ == '__main__':
    unittest.main()
