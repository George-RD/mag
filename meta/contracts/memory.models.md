---
node: mag.runtime.memory.models
---
# mag.runtime.memory.models contract

Embedding, generation, cross-encoding, and late-interaction reranking are
separate model roles behind explicit interfaces. The core quality baseline must
run locally without cloud credentials; remote and self-hosted adapters use the
same memory semantics.

Every production model profile declares its model ID, revision, checksums, role,
runtime, quantization, output dimensions, pooling, query/document handling,
maximum input length, licence, and expected local resource envelope. Embedding
interfaces distinguish query and document roles when a model's semantics
require it.

Persisted vectors carry an embedding-space identity. A profile change never
silently mixes vector spaces: MAG fails visibly and requires an explicit,
recoverable re-embedding migration. Missing models fail visibly and fall back
only when the caller explicitly permits it. Models with additional licence
conditions are opt-in profiles unless an accepted decision establishes another
default.
