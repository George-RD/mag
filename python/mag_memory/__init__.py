"""
mag-memory: PyPI wrapper for the mag MCP memory server.

On first run, the native binary is downloaded from GitHub Releases
for the current platform. Subsequent runs use the cached binary.
"""

import os
import shutil
import subprocess
import sys


__version__ = "0.1.0"

# Version of the Rust binary to download (kept in sync with __version__)
_BINARY_VERSION = "0.1.0"


def _binary_dir():
    # type: () -> str
    """Return the directory where the mag binary is stored."""
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin")


def _binary_name():
    # type: () -> str
    """Return the platform-appropriate binary filename."""
    if sys.platform == "win32":
        return "mag.exe"
    return "mag"


def _find_binary():
    # type: () -> str | None
    """Locate the mag binary: package dir first, then PATH."""
    # 1. Check package bin directory
    packaged = os.path.join(_binary_dir(), _binary_name())
    if os.path.isfile(packaged) and os.access(packaged, os.X_OK):
        return packaged

    # 2. Fall back to PATH
    found = shutil.which("mag")
    return found


def main():
    # type: () -> None
    """Entry point: find (or download) the mag binary and exec it."""
    binary = _find_binary()

    if binary is None:
        # Download on first run
        sys.stderr.write("mag: binary not found, downloading for this platform...\n")
        try:
            from mag_memory._download import download_binary

            binary = download_binary(_BINARY_VERSION)
        except Exception as exc:
            sys.stderr.write("mag: failed to download binary: {}\n".format(exc))
            sys.stderr.write(
                "mag: install manually from "
                "https://github.com/George-RD/mag/releases\n"
            )
            sys.exit(1)

    # Replace this process with the binary (Unix) or subprocess (Windows)
    args = [binary] + sys.argv[1:]

    if sys.platform != "win32":
        os.execvp(binary, args)
    else:
        # os.execvp is unreliable on Windows; use subprocess instead
        result = subprocess.run(args)
        sys.exit(result.returncode)
