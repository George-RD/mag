# Dead Code and Tech Debt Reconnaissance Report

> **Historical snapshot — 2026-04-14.** This reconnaissance describes the repository at that date; it is not current dead-code or architecture authority.
> Re-verify every count and conclusion against the code and use Cairn for live boundaries, decisions, and work status.

**Stronghold:** `/Users/george/repos/mag/docs/strongholds/recon-dead-code.md`  
**Compiled:** 2026-04-14  
**Status:** Read-only scout mission complete

## Summary

The MAG codebase is **exceptionally clean** with minimal dead code. All 43 explicit `#[allow(dead_code)]` suppressions are justified by:
- **Feature gates** (daemon-http, real-embeddings, sqlite-vec)
- **Cross-module visibility** (setup.rs cannot see test usage)
- **Optional implementations** (placeholder embedders, hot cache methods)

**Dead Code Scope:** ~15 functions / 600 LOC of intentionally-suppressed code; **0 orphaned/unreachable code detected**.

---

## Detailed Findings

### 1. Explicit Dead Code Suppressions: 43 Total

| Category | Count | Location | Rationale |
|----------|-------|----------|-----------|
| Feature-gated daemon | 8 | `src/main.rs`, `src/lib.rs`, `src/daemon.rs` | Only used when `daemon-http` feature enabled |
| Cross-module test usage | 4 | `src/tool_detection.rs`, `src/config_writer.rs`, `src/app_paths.rs` | Clippy cannot trace setup.rs/test usage |
| Optional embedder code | 3 | `src/memory_core/embedder.rs` | Placeholder implementations when ONNX disabled |
| Scoring params methods | 8 | `src/memory_core/storage/sqlite/mod.rs` | Used by grid search benchmarks and external APIs |
| Hot cache query method | 1 | `src/memory_core/storage/sqlite/hot_cache.rs` | Alternative query path (aliased by wrapper) |
| Benchmark utilities | 12+ | `benches/bench_utils/`, `benches/locomo/`, `benches/longmemeval/` | Test-harness and metric collection |
