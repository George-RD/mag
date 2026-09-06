---
node: mag.runtime.memory.storage.sqlite
status: in_progress
created: 2026-07-29
unblocked: 2026-08-28
---
# Implement Embedding-Space Migration

Unblocked by the role-aware embedding boundary, persisted embedding-space
identity checks, and the validated retriever profile contract completed through
PRs #414, #417, and #429.

Complete the model migration capability tracked by GitHub issue #89. Reuse the
existing `EmbeddingModel` profile/embedding-space identity boundary; do not add a
second model-name contract to the legacy `Embedder` compatibility path.

Provide a transactional batch `re-embed` path for the memory BLOB and vector
index. The operation needs dry-run and progress reporting, interruption-safe
recovery, dimension changes, cache invalidation, index repair, and a clear
rollback or backup path. MAG must never silently query a database containing
mixed or stale embedding spaces.

The CLI is the canonical command surface and the migration workflow belongs in
`LocalMemoryRuntime` (or a typed application workflow it owns). Any MCP exposure
must remain an optional thin transport over that same workflow rather than
calling `SqliteStorage` directly.

## Recoverable migration: PR #433

PR #433 implements dry-run affected-memory reporting, bounded embedding batches
with progress logs, a pre-migration backup, one transactional
BLOB/vector-index/identity migration, rollback on failure or interruption,
vector-index recreation for dimension changes, and a feature-minimal refusal
path when an existing vector index cannot be repaired without `sqlite-vec`.
The CLI is the command surface; MCP is unchanged.

Exact-head verification for implementation commit
`2c74b8d1f5cbf1e44bce84505f03a688a7fbe9e8` passed CI run `33257233088`,
including Rustfmt, Clippy, the full Rust suite, the no-default-features migration
test, benchmark gate, smoke test, wrappers, npm install, installer integrity, and
version consistency. Cairn architecture gate run `33257233085` also passed.

## Write safety and pinned production composition: PR #435

Implementation commit `be295658da3759d42cbacfde60f9f1b9c17e1780` applies the
transaction-level identity fence to store, content-update, batch-store, and
migration transactions. Normal CLI startup and `mag re-embed` share the pinned,
checksum-verified BGE profile. The unusable legacy migration entry point and
duplicate metadata helpers are removed. A custom same-dimension ONNX model is
rejected by the pinned BGE factory, rather than inheriting its identity.

Run `34024534055` preserved both RED proofs and a 1,000-test all-features GREEN.
Its later minimal-feature compile failure exposed an unconditional
`ReembedOptions` import. Run `34024862652` verifies the corrected feature gating
and passes the focused profile/migration/write regressions with and without
default features before committing the implementation. Final cleaned-head CI
and architecture results are linked from PR #435. The retained review is
`meta/reviews/pr435-embedding-space-write-safety.md`; operating guidance is
`docs/re-embedding.md`.

## Remaining boundary in this same todo

Keep this todo `in_progress` and issue #89 open. A runtime opened before another
process migrates can still query using its old embedder or return a cached
advanced-search result. The write fence does not establish live read/cache
safety. Migration is offline maintenance: stop all processes sharing the database
and restart fresh runtimes afterward.

The next implementation slice must prove stale semantic reads and cached
advanced-search reads fail visibly, including a migration racing with query
execution. Fence identity and vector reads to the same snapshot or generation;
a check before a later unguarded read is insufficient. Invalidate generation-bound
query/hot caches and cover both sqlite-vec and BLOB fallback. This is already
required by the storage contract and #89, not a new roadmap gap. Retrieval and
query-pipeline changes must pass the repository benchmark and local quality gates.
