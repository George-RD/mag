#!/usr/bin/env python3
"""Same as python_comparison.py but with embedding cache disabled."""
import os
import sys

sys.path.insert(0, os.path.expanduser("~/repos/omega-memory/src"))

# Disable embedding cache BEFORE any store/query calls
import omega.graphs as g  # noqa: E402

g._EMBEDDING_CACHE_MAX = 0

# Import the benchmark after patching
from python_comparison import main  # noqa: E402

if __name__ == "__main__":
    main()
