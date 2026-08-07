---
node: mag.quality.benchmarks
status: in_progress
created: 2026-08-07
---
# Close the retrieval benchmark gate gap

The engineering contract requires benchmark verification for retrieval, scoring,
reranking, and SQLite query-pipeline changes. The current CI-only path filter can
miss top-level implementation files including:

- `src/memory_core/scoring.rs`;
- `src/memory_core/scoring_strategy.rs`;
- `src/memory_core/reranker.rs`;
- `src/memory_core/retrieval_strategy.rs`.

It also watches `src/memory_core/scoring/**`, which does not match the actual
`scoring.rs` file.

## Acceptance criteria

- [ ] One repository-owned classifier defines benchmark-relevant paths.
- [ ] CI consumes that classifier instead of maintaining an independent YAML
  path list.
- [ ] Local development guidance consumes the same classifier.
- [ ] Contract tests cover every governed retrieval/scoring/reranking path and
  representative unrelated paths.
- [ ] Classifier errors fail CI rather than silently skipping the benchmark.
- [ ] The benchmark gate still runs for this PR because the classifier itself
  identifies its governed source changes correctly.
- [ ] Exact-head CI and Cairn pass before merge.

## Boundary

This slice changes verification eligibility only. It does not alter retrieval,
scoring, reranking, benchmark thresholds, or production runtime behavior.
