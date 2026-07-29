---
node: mag.runtime.memory.storage.sqlite
status: blocked
created: 2026-07-29
---
# Implement Embedding-Space Migration

Blocked by `todo.define-role-aware-retriever-profiles`.

Complete the model migration capability tracked by GitHub issue #89. Persist the
profile and embedding-space identity that produced stored vectors, detect
mismatches on open, and provide a transactional batch `re-embed` path for the
memory BLOB and vector index.

The operation needs dry-run and progress reporting, interruption-safe recovery,
dimension changes, cache invalidation, index repair, and a clear rollback or
backup path. MAG must never silently query a database containing mixed or stale
embedding spaces.
