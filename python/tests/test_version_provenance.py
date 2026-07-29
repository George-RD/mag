import importlib.metadata
import os
import sys
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

    @unittest.skipIf(os.name == "nt", "Unix exec path is tested on Linux CI")
    def test_main_downloads_the_distribution_version(self):
        with (
            mock.patch.object(mag_memory, "_find_binary", return_value=None),
            mock.patch.object(
                mag_memory,
                "_binary_version",
                return_value="9.8.7",
                create=True,
            ),
            mock.patch(
                "mag_memory._download.download_binary",
                return_value="/tmp/mag",
            ) as download_binary,
            mock.patch.object(mag_memory.os, "execvp") as execvp,
        ):
            mag_memory.main()

        download_binary.assert_called_once_with("9.8.7")
        execvp.assert_called_once_with("/tmp/mag", ["/tmp/mag", *sys.argv[1:]])


if __name__ == "__main__":
    unittest.main()
