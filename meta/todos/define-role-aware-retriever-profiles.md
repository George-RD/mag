---
node: mag.runtime.memory.models
status: blocked
created: 2026-07-29
---
# Define Role-Aware Retriever Profiles

Blocked by `todo.select-production-composition-root`.

Define one model-profile contract for dense encoders and rerankers. It must
record model ID, revision, checksum, role, runtime, quantization, dimensions,
pooling, query/document prefixes, maximum input length, licence, and local
resource expectations.

Change the embedding boundary so models can distinguish query and document
encoding without leaking model-specific prefixes into ingestion or retrieval
call sites. Keep generation, dense embedding, cross-encoding, and late
interaction as separate roles even when they use the same model family.
