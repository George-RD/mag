---
node: mag.runtime.memory.models
status: in_progress
created: 2026-07-29
---
# Define Role-Aware Retriever Profiles

The production composition root is selected, so the former blocker is closed.
The first implementation slice adds a typed query/document embedding boundary,
preserves role-neutral embedders through one compatibility adapter, and routes
SQLite ingestion, updates, batch writes, semantic search, advanced search, and
query decomposition through explicit input roles.

The next slice persists an embedding-space identity with each SQLite database and
rejects reopening it through an explicit `EmbeddingModel` whose identity differs,
even when vector dimensions match. This makes an incompatible profile change a
visible migration requirement instead of silently mixing vector spaces. The
legacy role-neutral `Embedder` path remains compatibility-only; profile changes
must use the explicit embedding-model boundary.

Remaining work is to define one immutable model-profile contract for dense
encoders and rerankers. It must record model ID, revision, checksum, role,
runtime, quantization, dimensions, pooling, query/document transformation,
maximum input length, licence, and local resource expectations. An explicit,
recoverable re-embedding migration is required before profile switching can ship.

Keep generation, dense embedding, cross-encoding, and late interaction as
separate roles even when they use the same model family.
