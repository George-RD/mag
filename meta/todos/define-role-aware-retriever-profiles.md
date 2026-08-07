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

Remaining work is to define one immutable model-profile contract for dense
encoders and rerankers. It must record model ID, revision, checksum, role,
runtime, quantization, dimensions, pooling, query/document transformation,
maximum input length, licence, and local resource expectations. Persisted
embedding-space identity is required before a profile change can ship.

Keep generation, dense embedding, cross-encoding, and late interaction as
separate roles even when they use the same model family.
