#!/usr/bin/env python3
"""Same as python_comparison.py but with embedding cache disabled."""
import os
import sys

omega_repo = os.path.expanduser(os.environ.get("OMEGA_REPO", "~/repos/omega-memory"))
omega_src = os.path.join(omega_repo, "src")
if not os.path.isdir(omega_src):
    raise SystemExit(
        f"omega-memory repo not found at {omega_repo} "
        f"(resolved OMEGA_REPO={os.environ.get('OMEGA_REPO', '~/repos/omega-memory')})"
    )
sys.path.insert(0, omega_src)

# Disable embedding cache BEFORE any store/query calls
import omega.graphs as g  # noqa: E402

g._EMBEDDING_CACHE_MAX = 0

# Import the benchmark after patching
from python_comparison import main  # noqa: E402

if __name__ == "__main__":
    main()
