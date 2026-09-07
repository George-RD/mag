---
node: mag.runtime.memory.storage.sqlite
status: done
created: 2026-07-29
unblocked: 2026-08-28
completed: 2026-09-08
---
# Implement Embedding-Space Migration

Complete the model migration capability tracked by GitHub issue #89. The
role-aware embedding boundary, persisted embedding-space identity checks, and
validated retriever profile contract landed through PRs #414, #417, and #429.
The recoverable migration, write safety, pinned production composition, and
read/cache generation safety are now implemented by the slices recorded below.

The CLI remains the canonical command surface through `LocalMemoryRuntime`.
Migration reuses `EmbeddingModel`; MCP remains an optional thin transport over
the same runtime. This does not introduce arbitrary CLI model selection, live
profile hot-swapping, or a second legacy embedder/model-name contract.

## Recoverable migration: PRs #433 and #434

The migration implements dry-run affected-memory reporting, bounded embedding
batches with progress logs, a pre-migration backup, one transactional
BLOB/vector-index/identity migration, rollback on failure or interruption,
vector-index recreation for dimension changes, and a feature-minimal refusal
path when an existing vector index cannot be repaired without `sqlite-vec`.
The CLI is the command surface; MCP is unchanged.

Exact-head verification for implementation commit
`2c74b8d1f5cbf1e44bce84505f03a688a7fbe9e8` passed CI run `33257233088`,
including Rustfmt, Clippy, the full Rust suite, the no-default-features migration
test, benchmark gate, smoke test, wrappers, npm install, installer integrity, and
version consistency. Cairn architecture gate run `33257233085` also passed.
PR #434 carries the final migration and backup-integrity follow-up.

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
gates](https://github.com/George-RD/mag/actions/runs/34102423945) records RED before
the fix and the ensuing focused/full gates. This run verifies a patched worktree;
PR #435 separately records final exact-head CI after the temporary runner was
removed.

That slice deliberately left live read/cache safety open: an already-running
process could still query with its old embedder or return a cached advanced-search
result after another process migrated. The read/cache slice below closes that
remaining requirement; the earlier review's warning describes its historical
boundary, not the current implementation.

## Read/cache generation safety: 8 September 2026

Each SQLite pool binds to its startup model identity and persisted embedding
generation. Semantic and similar-memory reads pin identity, generation, vectors,
and hydration to one SQLite snapshot. Advanced-search candidate, fusion, graph,
and decomposition phases verify the same generation binding. A final live check
also gates cache hits and completed results. Rejection clears query and hot
caches; hot-cache refresh uses a verified snapshot. Successful migration advances
generation in its existing transaction. Dry-run, no-op, and rollback do not.
Returning from model A to B and back to A cannot revive an old runtime's caches.

Test-only head `e99964f313c3306d86a39d4aad89011f1485f51b` reproduced four assertion
failures in both BLOB fallback and sqlite-vec configurations in [RED run
34158983897](https://github.com/George-RD/mag/actions/runs/34158983897). Additional
regressions cover migration after snapshot acquisition, between advanced-search
phases, cache invalidation, malformed generation metadata, and generation overflow
rolling back vectors.

[GREEN run 34159459046](https://github.com/George-RD/mag/actions/runs/34159459046)
verified the patched worktree before publishing implementation commit
`5f94f333dcd6a27db43ff43df04b4a41bf68afdd`: 1,026 all-feature Rust tests,
17 focused migration/read/cache tests in each storage configuration, Rustfmt,
strict Clippy, the retrieval benchmark, and Cairn scan/hooks passed. The benchmark
used the repository's two-sample gate: 91.3% word overlap against the recorded
90.1% ten-sample baseline. These differently sized samples are not a quality
improvement claim. The existing stale-methodology warning remains visible.

The durable review and rerun commands are in
`meta/reviews/embedding-space-read-safety.md`. Cleanup removes the temporary
workflow and patch carrier and separates advanced-search tests without changing
behavior. The pull request records verification of its final cleaned head;
intermediate worktree evidence does not replace final-head CI or authorize merge.

## Operating boundary

Stop all processes sharing the database, migrate, then start fresh runtimes.
These read/write guards make accidental overlap fail visibly; they do not make
in-place database-file replacement or profile hot-swapping supported operations.
Raw retrieval and metadata-only operations remain available. The separate local
memory intelligence evaluation harness remains its own existing Cairn todo, not
an unfinished requirement of #89.
