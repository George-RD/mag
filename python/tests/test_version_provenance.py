import importlib.metadata
import unittest

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

    def test_wrapper_has_no_independent_binary_version_constant(self):
        self.assertFalse(hasattr(mag_memory, "_BINARY_VERSION"))


if __name__ == "__main__":
    unittest.main()
