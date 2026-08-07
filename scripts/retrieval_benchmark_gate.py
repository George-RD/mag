#!/usr/bin/env python3
"""Classify whether a change requires MAG's retrieval benchmark gate.

The repository contract is intentionally owned here rather than duplicated in
GitHub Actions YAML. CI calls Git diff mode; developers may use either Git diff
mode or explicit paths.

Examples:

    python3 scripts/retrieval_benchmark_gate.py --base main --head HEAD
    python3 scripts/retrieval_benchmark_gate.py --paths src/memory_core/scoring.rs

The command prints exactly ``true`` or ``false`` to stdout. Use ``--explain`` to
print matching paths to stderr without changing the machine-readable output.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from collections.abc import Iterable, Sequence


# A stem matches the current file, related sibling files, or a future module
# directory. For example, ``.../scoring`` matches scoring.rs, scoring_tests.rs,
# and scoring/mod.rs without requiring a CI edit when the module is reorganized.
BENCHMARK_RELEVANT_STEMS: tuple[str, ...] = (
    "src/memory_core/scoring",
    "src/memory_core/scoring_strategy",
    "src/memory_core/reranker",
    "src/memory_core/retrieval_strategy",
    "src/memory_core/storage/sqlite/search",
    "src/memory_core/storage/sqlite/advanced",
)

BENCHMARK_RELEVANT_PREFIXES: tuple[str, ...] = (
    "src/memory_core/storage/sqlite/pipeline/",
)


def normalize_repo_path(path: str) -> str:
    """Return a stable repository-relative path representation."""

    normalized = path.strip().replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    while "//" in normalized:
        normalized = normalized.replace("//", "/")
    return normalized


def _matches_stem(path: str, stem: str) -> bool:
    return (
        path == stem
        or path.startswith(f"{stem}.")
        or path.startswith(f"{stem}_")
        or path.startswith(f"{stem}/")
    )


def is_benchmark_relevant(path: str) -> bool:
    """Return whether one repository path is benchmark governed."""

    normalized = normalize_repo_path(path)
    return any(_matches_stem(normalized, stem) for stem in BENCHMARK_RELEVANT_STEMS) or any(
        normalized.startswith(prefix) for prefix in BENCHMARK_RELEVANT_PREFIXES
    )


def benchmark_relevant_paths(paths: Iterable[str]) -> list[str]:
    """Return normalized matching paths, deduplicated in input order."""

    matches: list[str] = []
    seen: set[str] = set()
    for path in paths:
        normalized = normalize_repo_path(path)
        if normalized in seen or not is_benchmark_relevant(normalized):
            continue
        seen.add(normalized)
        matches.append(normalized)
    return matches


def requires_benchmark(paths: Iterable[str]) -> bool:
    """Return whether any supplied path requires benchmark verification."""

    return bool(benchmark_relevant_paths(paths))


def changed_paths(base: str, head: str) -> list[str]:
    """Return changed paths between two Git revisions.

    Rename detection is disabled so both the old deletion and new addition remain
    visible. Moving a governed retrieval file outside a governed path therefore
    cannot silently skip the benchmark.
    """

    completed = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", f"{base}...{head}"],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(
            f"git diff failed for {base}...{head}"
            + (f": {detail}" if detail else "")
        )
    return [line for line in completed.stdout.splitlines() if line.strip()]


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--paths",
        nargs="*",
        help="classify explicit repository paths instead of a Git diff",
    )
    parser.add_argument("--base", help="base Git revision for three-dot diff mode")
    parser.add_argument("--head", help="head Git revision for three-dot diff mode")
    parser.add_argument(
        "--explain",
        action="store_true",
        help="write matching governed paths to stderr",
    )
    args = parser.parse_args(argv)

    explicit_paths = args.paths is not None
    git_diff = args.base is not None or args.head is not None
    if explicit_paths and git_diff:
        parser.error("use either --paths or --base/--head, not both")
    if not explicit_paths and not (args.base and args.head):
        parser.error("provide --paths or both --base and --head")
    if git_diff and not (args.base and args.head):
        parser.error("--base and --head must be provided together")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        paths = args.paths if args.paths is not None else changed_paths(args.base, args.head)
        matches = benchmark_relevant_paths(paths)
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    if args.explain:
        if matches:
            print("retrieval benchmark required by:", file=sys.stderr)
            for path in matches:
                print(f"  {path}", file=sys.stderr)
        else:
            print("no benchmark-governed paths changed", file=sys.stderr)

    print("true" if matches else "false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
