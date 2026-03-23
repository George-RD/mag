"""
Download the correct mag binary for the current platform from GitHub Releases.
"""

import io
import os
import platform
import stat
import sys
import tarfile
import zipfile

try:
    from urllib.request import urlopen, Request
    from urllib.error import URLError, HTTPError
except ImportError:
    # Python 2 fallback (shouldn't happen with >=3.8, but defensive)
    from urllib2 import urlopen, Request, URLError, HTTPError  # type: ignore[no-redef]


_GITHUB_RELEASE_URL = (
    "https://github.com/George-RD/mag/releases/download/"
    "v{version}/mag-{target}.{ext}"
)

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
        resp = urlopen(req, timeout=120)
        return resp.read()
    except HTTPError as exc:
        raise RuntimeError(
            "HTTP {} downloading {}: {}".format(exc.code, url, exc.reason)
        )
    except URLError as exc:
        raise RuntimeError("Failed to download {}: {}".format(url, exc.reason))


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
    """Download the mag binary for this platform and return its path.

    Args:
        version: The version string (e.g. "0.1.0")

    Returns:
        Absolute path to the downloaded binary.
    """
    target, ext = _detect_target()
    url = _GITHUB_RELEASE_URL.format(version=version, target=target, ext=ext)

    print("mag: downloading {} ...".format(url))
    sys.stdout.flush()
    data = _download_url(url)
    print("mag: downloaded {:.1f} MB".format(len(data) / (1024.0 * 1024.0)))
    sys.stdout.flush()

    # Ensure destination directory exists
    from mag_memory import _binary_dir

    dest_dir = _binary_dir()
    os.makedirs(dest_dir, exist_ok=True)

    # Extract
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
