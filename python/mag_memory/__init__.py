"""
mag-memory: PyPI wrapper for the mag MCP memory server.

On first run, the native binary is downloaded from GitHub Releases
for the current platform. Subsequent runs use the cached binary.
"""

import os
import shutil
import subprocess
import sys


__version__ = "0.1.2"

# Version of the Rust binary to download (kept in sync with __version__)
_BINARY_VERSION = "0.1.2"


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


def _is_rust_binary(path):
    # type: (str) -> bool
    """Check if a file is a native binary (not a script wrapper)."""
    try:
        with open(path, "rb") as f:
            header = f.read(4)
            # Mach-O (macOS), ELF (Linux), MZ (Windows PE)
            return header[:4] in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe",
                                  b"\x7fELF", b"MZ\x90\x00", b"MZ\x00\x00")
    except (IOError, OSError):
        return False


def _find_binary():
    # type: () -> str | None
    """Locate the mag binary: package dir first, then PATH."""
    # 1. Check package bin directory
    packaged = os.path.join(_binary_dir(), _binary_name())
    if os.path.isfile(packaged) and os.access(packaged, os.X_OK):
        return packaged

    # 2. Fall back to PATH — but only accept real binaries, not script wrappers
    #    (avoids infinite recursion when uv/pipx wrapper is on PATH)
    found = shutil.which("mag")
    if found and _is_rust_binary(found):
        return found
    return None


def main():
    # type: () -> None
    """Entry point: find (or download) the mag binary and exec it."""
    binary = _find_binary()

    if binary is None:
        # Download on first run — print to stdout and flush so user sees progress
        print("mag: binary not found, downloading for this platform...")
        sys.stdout.flush()
        try:
            from mag_memory._download import download_binary

            binary = download_binary(_BINARY_VERSION)
            print("mag: ready!")
            sys.stdout.flush()
        except Exception as exc:
            print("mag: failed to download binary: {}".format(exc))
            print(
                "mag: install manually from "
                "https://github.com/George-RD/mag/releases"
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
