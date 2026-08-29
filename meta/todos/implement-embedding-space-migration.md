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

## Current implementation evidence

PR #433 implements the recoverable migration slice through `LocalMemoryRuntime`:
dry-run affected-memory reporting, bounded embedding batches with progress logs,
a pre-migration backup, one transactional BLOB/vector-index/identity migration,
rollback on failure or interruption, vector-index recreation for dimension
changes, and a feature-minimal refusal path when an existing vector index cannot
be repaired without `sqlite-vec`. The CLI is the command surface; MCP is unchanged.

Exact-head verification for implementation commit
`2c74b8d1f5cbf1e44bce84505f03a688a7fbe9e8` passed CI run `33257233088`,
including Rustfmt, Clippy, the full Rust suite, the no-default-features migration
test, benchmark gate, smoke test, wrappers, npm install, installer integrity, and
version consistency. Cairn architecture gate run `33257233085` also passed.

Keep this todo `in_progress`: the legacy `Embedder` path deliberately preserves
its compatibility-only dimension identity and must not fabricate validated model
metadata. Production profile switching therefore remains on the explicit
`EmbeddingModel` boundary once a pinned profile-backed composition path is
selected; this PR does not claim that remaining profile-selection work is done.
