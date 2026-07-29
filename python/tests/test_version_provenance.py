import importlib.metadata
import unittest
from unittest import mock

import mag_memory


class VersionProvenanceTests(unittest.TestCase):
    def test_public_version_matches_installed_distribution(self):
        self.assertEqual(
            importlib.metadata.version("mag-memory"),
            mag_memory.__version__,
        )

    def test_binary_version_matches_installed_distribution(self):
        self.assertEqual(
            importlib.metadata.version("mag-memory"),
            mag_memory._binary_version(),
        )

    def test_binary_version_fails_closed_without_distribution_metadata(self):
        with mock.patch.object(mag_memory, "__version__", "0+unknown"):
            with self.assertRaisesRegex(RuntimeError, "installed package metadata"):
                mag_memory._binary_version()

    def test_wrapper_has_no_independent_binary_version_constant(self):
        self.assertFalse(hasattr(mag_memory, "_BINARY_VERSION"))


if __name__ == "__main__":
    unittest.main()
