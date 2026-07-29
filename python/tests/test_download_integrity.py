import hashlib
import os
import stat
import tempfile
import unittest
from unittest import mock

import mag_memory
from mag_memory import _download


_TARGET = "x86_64-unknown-linux-gnu"
_ARCHIVE_NAME = "mag-{}.tar.gz".format(_TARGET)


class ChecksumManifestTests(unittest.TestCase):
    def test_selects_the_exact_archive_entry(self):
        expected = "a" * 64
        manifest = (
            "{}  mag-aarch64-unknown-linux-gnu.tar.gz\n"
            "{}  {}\n"
        ).format("b" * 64, expected, _ARCHIVE_NAME)

        self.assertEqual(
            expected,
            _download._parse_checksum_manifest(manifest.encode("ascii"), _ARCHIVE_NAME),
        )

    def test_rejects_a_missing_archive_entry(self):
        manifest = "{}  mag-aarch64-unknown-linux-gnu.tar.gz\n".format("a" * 64)

        with self.assertRaisesRegex(RuntimeError, "No checksum entry"):
            _download._parse_checksum_manifest(manifest.encode("ascii"), _ARCHIVE_NAME)

    def test_rejects_duplicate_archive_entries(self):
        manifest = (
            "{}  {}\n"
            "{}  {}\n"
        ).format("a" * 64, _ARCHIVE_NAME, "b" * 64, _ARCHIVE_NAME)

        with self.assertRaisesRegex(RuntimeError, "Duplicate checksum entries"):
            _download._parse_checksum_manifest(manifest.encode("ascii"), _ARCHIVE_NAME)

    def test_rejects_a_malformed_digest(self):
        manifest = "not-a-sha256  {}\n".format(_ARCHIVE_NAME)

        with self.assertRaisesRegex(RuntimeError, "Malformed checksum"):
            _download._parse_checksum_manifest(manifest.encode("ascii"), _ARCHIVE_NAME)


class ArchiveChecksumTests(unittest.TestCase):
    def test_accepts_matching_archive_bytes(self):
        archive = b"verified archive"
        expected = hashlib.sha256(archive).hexdigest()

        _download._verify_archive_checksum(archive, expected, _ARCHIVE_NAME)

    def test_rejects_mismatched_archive_bytes(self):
        archive = b"tampered archive"

        with self.assertRaisesRegex(RuntimeError, "Checksum mismatch"):
            _download._verify_archive_checksum(archive, "0" * 64, _ARCHIVE_NAME)


class VerifiedDownloadFlowTests(unittest.TestCase):
    def test_checksum_failure_happens_before_install_side_effects(self):
        archive = b"tampered archive"
        manifest = "{}  {}\n".format("0" * 64, _ARCHIVE_NAME).encode("ascii")

        with tempfile.TemporaryDirectory() as tmp:
            dest_dir = os.path.join(tmp, "bin")
            with mock.patch.object(
                _download, "_detect_target", return_value=(_TARGET, "tar.gz")
            ), mock.patch.object(
                _download, "_download_url", side_effect=[manifest, archive]
            ) as fetch, mock.patch.object(
                mag_memory, "_binary_dir", return_value=dest_dir
            ), mock.patch.object(
                _download, "_extract_tar_gz"
            ) as extract:
                with self.assertRaisesRegex(RuntimeError, "Checksum mismatch"):
                    _download.download_binary("1.2.3")

            self.assertEqual(
                [
                    "https://github.com/George-RD/mag/releases/download/v1.2.3/checksums.txt",
                    "https://github.com/George-RD/mag/releases/download/v1.2.3/{}".format(
                        _ARCHIVE_NAME
                    ),
                ],
                [call.args[0] for call in fetch.call_args_list],
            )
            extract.assert_not_called()
            self.assertFalse(os.path.exists(dest_dir))

    @unittest.skipIf(os.name == "nt", "Unix executable permissions are tested on Linux CI")
    def test_verified_archive_is_installed_after_checksum_validation(self):
        archive = b"verified archive"
        digest = hashlib.sha256(archive).hexdigest()
        manifest = "{}  {}\n".format(digest, _ARCHIVE_NAME).encode("ascii")

        with tempfile.TemporaryDirectory() as tmp:
            dest_dir = os.path.join(tmp, "bin")

            def extract(data, destination):
                self.assertEqual(archive, data)
                self.assertTrue(os.path.isdir(destination))
                path = os.path.join(destination, "mag")
                with open(path, "wb") as binary:
                    binary.write(b"binary")
                return path

            with mock.patch.object(
                _download, "_detect_target", return_value=(_TARGET, "tar.gz")
            ), mock.patch.object(
                _download, "_download_url", side_effect=[manifest, archive]
            ), mock.patch.object(
                mag_memory, "_binary_dir", return_value=dest_dir
            ), mock.patch.object(
                _download, "_extract_tar_gz", side_effect=extract
            ) as extract_archive:
                binary_path = _download.download_binary("1.2.3")

            self.assertEqual(os.path.join(dest_dir, "mag"), binary_path)
            extract_archive.assert_called_once_with(archive, dest_dir)
            self.assertTrue(os.stat(binary_path).st_mode & stat.S_IXUSR)


if __name__ == "__main__":
    unittest.main()
