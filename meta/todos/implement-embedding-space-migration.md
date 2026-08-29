---
node: mag.runtime.memory.storage.sqlite
status: open
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
