---
node: mag.quality.benchmarks
status: done
created: 2026-08-07
completed: 2026-08-07
---
# Close the retrieval benchmark gate gap

The engineering contract requires benchmark verification for retrieval, scoring,
reranking, and SQLite query-pipeline changes. The previous CI-only path filter
could miss top-level implementation files including:

- `src/memory_core/scoring.rs`;
- `src/memory_core/scoring_strategy.rs`;
- `src/memory_core/reranker.rs`;
- `src/memory_core/retrieval_strategy.rs`.

It also watched `src/memory_core/scoring/**`, which did not match the actual
`scoring.rs` file.

## Acceptance criteria

- [x] One repository-owned classifier defines benchmark-relevant paths.
- [x] CI consumes that classifier instead of maintaining an independent YAML
  path list.
- [x] Local development guidance consumes the same classifier.
- [x] Contract tests cover every governed retrieval/scoring/reranking path and
  representative unrelated paths.
- [x] Classifier errors fail CI rather than silently skipping the benchmark.
- [x] Classifier contract tests run on every pull request; the full benchmark
  runs only when governed production paths changed.
- [x] Exact-head CI and Cairn passed before merge.

## TDD evidence

CI run #1132 (`31183110392`) failed its Benchmark Gate at
`c0923d1f5c55540f25b3836550e26d3c1dae8eea` because the contract test could not
load the deliberately absent `scripts/retrieval_benchmark_gate.py`. Detection and
the benchmark were skipped after that red test.

The implementation at `2891145e5ac2ff50b03fb4ecb595128aea308e80` adds the
classifier with normalized, deduplicated path matching and rename-safe three-dot
Git diffing. CI and local guidance now call that same script.

The first complete implementation head,
`b9e1f58c5f0f7891e7a18ae64c55865777b25d37`, passed CI run #1134
(`31183808674`) and Cairn run #425 (`31183809950`). Classifier contract tests
passed and correctly classified the verification-only pull request as not
requiring the expensive retrieval benchmark. Formatting, Clippy, all-feature
Rust tests, smoke coverage, npm installation, Python wrappers, version checks,
and installer integrity also passed.

## Boundary

This slice changes verification eligibility only. It does not alter retrieval,
scoring, reranking, benchmark thresholds, or production runtime behavior.
