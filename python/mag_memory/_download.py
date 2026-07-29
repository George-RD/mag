"""
Download and verify the correct mag binary for the current platform.
"""

import hashlib
import hmac
import io
import os
import platform
import stat
import sys
import tarfile
import zipfile

try:
    from urllib.error import HTTPError, URLError
    from urllib.request import Request, urlopen
except ImportError:
    # Python 2 fallback (shouldn't happen with >=3.8, but defensive)
    from urllib2 import HTTPError, Request, URLError, urlopen  # type: ignore[no-redef]


_GITHUB_RELEASE_URL = (
    "https://github.com/George-RD/mag/releases/download/"
    "v{version}/mag-{target}.{ext}"
)
_GITHUB_CHECKSUMS_URL = (
    "https://github.com/George-RD/mag/releases/download/v{version}/checksums.txt"
)
_HEX_DIGITS = frozenset("0123456789abcdef")

# Mapping: (sys.platform, platform.machine()) -> Rust target triple
_TARGET_MAP = {
    ("linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("linux", "aarch64"): "aarch64-unknown-linux-gnu",
    ("linux", "arm64"): "aarch64-unknown-linux-gnu",
    ("darwin", "x86_64"): "x86_64-apple-darwin",
    ("darwin", "arm64"): "aarch64-apple-darwin",
    ("darwin", "aarch64"): "aarch64-apple-darwin",
    ("win32", "AMD64"): "x86_64-pc-windows-msvc",
    ("win32", "x86_64"): "x86_64-pc-windows-msvc",
}


def _detect_target():
    # type: () -> tuple[str, str]
    """Detect the Rust target triple and archive extension for this platform.

    Returns:
        (target_triple, archive_extension)
    """
    plat = sys.platform
    # Normalize platform string
    if plat.startswith("linux"):
        plat = "linux"

    machine = platform.machine()

    key = (plat, machine)
    target = _TARGET_MAP.get(key)
    if target is None:
        raise RuntimeError(
            "Unsupported platform: {} / {} (machine={})".format(
                sys.platform, platform.system(), machine
            )
        )

    ext = "zip" if plat == "win32" else "tar.gz"
    return target, ext


def _download_url(url):
    # type: (str) -> bytes
    """Download a URL and return its content as bytes."""
    req = Request(url, headers={"User-Agent": "mag-memory-pypi-installer"})
    try:
        with urlopen(req, timeout=120) as resp:
            return resp.read()
    except HTTPError as exc:
        raise RuntimeError(
            "HTTP {} downloading {}: {}".format(exc.code, url, exc.reason)
        )
    except URLError as exc:
        raise RuntimeError("Failed to download {}: {}".format(url, exc.reason))


def _parse_checksum_manifest(data, archive_name):
    # type: (bytes, str) -> str
    """Return the one SHA-256 digest declared for an exact archive name."""
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise RuntimeError("Malformed checksum manifest: not valid UTF-8") from exc

    matches = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue

        parts = line.split(None, 1)
        if len(parts) != 2:
            raise RuntimeError(
                "Malformed checksum manifest line {}: expected digest and filename".format(
                    line_number
                )
            )

        digest, filename = parts
        digest = digest.lower()
        filename = filename.strip()
        if filename.startswith("*"):
            filename = filename[1:]

        if len(digest) != 64 or any(char not in _HEX_DIGITS for char in digest):
            raise RuntimeError(
                "Malformed checksum on line {} for '{}'".format(line_number, filename)
            )

        if filename == archive_name:
            matches.append(digest)

    if not matches:
        raise RuntimeError("No checksum entry for '{}'".format(archive_name))
    if len(matches) != 1:
        raise RuntimeError("Duplicate checksum entries for '{}'".format(archive_name))
    return matches[0]


def _verify_archive_checksum(data, expected_digest, archive_name):
    # type: (bytes, str, str) -> None
    """Fail unless archive bytes match the expected SHA-256 digest."""
    actual_digest = hashlib.sha256(data).hexdigest()
    if not hmac.compare_digest(actual_digest, expected_digest.lower()):
        raise RuntimeError(
            "Checksum mismatch for '{}': expected {}, got {}".format(
                archive_name, expected_digest, actual_digest
            )
        )


def _extract_tar_gz(data, dest_dir):
    # type: (bytes, str) -> str
    """Extract a .tar.gz archive, find the mag binary, place it in dest_dir."""
    binary_name = "mag"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        # Find the mag binary in the archive
        members = tar.getnames()
        binary_member = None
        for name in members:
            basename = os.path.basename(name)
            if basename == binary_name:
                binary_member = name
                break

        if binary_member is None:
            raise RuntimeError(
                "Could not find '{}' in archive. Contents: {}".format(
                    binary_name, members
                )
            )

        # Extract just the binary
        member = tar.getmember(binary_member)
        fileobj = tar.extractfile(member)
        if fileobj is None:
            raise RuntimeError("Could not extract '{}'".format(binary_member))

        dest_path = os.path.join(dest_dir, binary_name)
        with open(dest_path, "wb") as f:
            f.write(fileobj.read())

    return dest_path


def _extract_zip(data, dest_dir):
    # type: (bytes, str) -> str
    """Extract a .zip archive, find the mag binary, place it in dest_dir."""
    binary_name = "mag.exe"
    with zipfile.ZipFile(io.BytesIO(data)) as zf:
        names = zf.namelist()
        binary_member = None
        for name in names:
            basename = os.path.basename(name)
            if basename == binary_name:
                binary_member = name
                break

        if binary_member is None:
            raise RuntimeError(
                "Could not find '{}' in archive. Contents: {}".format(
                    binary_name, names
                )
            )

        dest_path = os.path.join(dest_dir, binary_name)
        with open(dest_path, "wb") as f:
            f.write(zf.read(binary_member))

    return dest_path


def download_binary(version):
    # type: (str) -> str
    """Download a verified mag binary for this platform and return its path.

    Args:
        version: The version string (e.g. "0.1.0")

    Returns:
        Absolute path to the downloaded binary.
    """
    target, ext = _detect_target()
    archive_name = "mag-{}.{}".format(target, ext)
    checksum_url = _GITHUB_CHECKSUMS_URL.format(version=version)
    archive_url = _GITHUB_RELEASE_URL.format(
        version=version,
        target=target,
        ext=ext,
    )

    print("mag: fetching release checksums ...")
    sys.stdout.flush()
    manifest = _download_url(checksum_url)
    expected_digest = _parse_checksum_manifest(manifest, archive_name)

    print("mag: downloading {} ...".format(archive_url))
    sys.stdout.flush()
    data = _download_url(archive_url)
    print("mag: downloaded {:.1f} MB".format(len(data) / (1024.0 * 1024.0)))
    sys.stdout.flush()

    _verify_archive_checksum(data, expected_digest, archive_name)
    print("mag: checksum verified")
    sys.stdout.flush()

    # Create the destination only after the downloaded archive is verified.
    from mag_memory import _binary_dir

    dest_dir = _binary_dir()
    os.makedirs(dest_dir, exist_ok=True)

    if ext == "zip":
        binary_path = _extract_zip(data, dest_dir)
    else:
        binary_path = _extract_tar_gz(data, dest_dir)

    # Make executable (Unix)
    if sys.platform != "win32":
        st = os.stat(binary_path)
        os.chmod(binary_path, st.st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    print("mag: installed to {}".format(binary_path))
    sys.stdout.flush()
    return binary_path


if __name__ == "__main__":
    # Allow running directly: python -m mag_memory._download [version]
    ver = sys.argv[1] if len(sys.argv) > 1 else "0.1.0"
    path = download_binary(ver)
    print("Downloaded: {}".format(path))
