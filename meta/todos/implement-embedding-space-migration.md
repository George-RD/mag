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

## PR #435 review follow-up: 7 September 2026

Fixed the pinned-cache nested Tokio runtime panic with synchronous cached-file
verification and an isolated download runtime for synchronous calls from Tokio.
Added fourteen hermetic artifact regressions for cached/cold runtime contexts,
checksum recovery/rejection, replaced-file re-verification, and sidecar identity.
Normal CI now grants read-only contents access. [Review TDD and engineering
gates](https://github.com/George-RD/mag/actions/runs/34102423945) records RED before the fix and the ensuing focused/full gates.
This run verifies a patched worktree; PR #435 separately records final exact-head
CI after the temporary runner is removed. Existing read/cache work below remains
in progress; this follow-up adds no todo or new public runtime surface.

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


## Read/cache safety implementation: 8 September 2026

The `agent/embedding-space-read-fence` slice binds each SQLite pool to its startup
model identity and persisted embedding generation. Semantic and similar-memory
reads pin metadata, vectors, and hydration to one SQLite snapshot. Advanced-search
candidate, fusion, graph, and decomposition phases use the same generation
binding; a live check also gates cached/final results. Rejection clears the query
and hot caches, and hot-cache refresh uses a verified snapshot. Successful
migration advances generation transactionally; dry-run, no-op, and rollback do not.
The CLI/runtime boundary and model defaults are unchanged.

Test-only head `e99964f313c3306d86a39d4aad89011f1485f51b` reproduced four assertion
failures in both BLOB fallback and sqlite-vec configurations in run `34158983897`:
stale semantic results, a confirmed advanced-search cache hit, A-to-B-to-A cache
reuse, and migration while query embedding is paused. Additional coverage pins
the read-snapshot race and a migration between advanced-search phases. Exact-head
engineering, benchmark, architecture, and review evidence belongs in the linked
PR; this note does not claim those later checks have passed yet.

Keep this todo and #89 in progress until the complete cleaned-head verification
and remaining adoption/evaluation obligations are reconciled. Offline stop,
migrate, restart guidance remains in force; this is not a live profile hot-swap
or permission to replace a database file under running processes.
