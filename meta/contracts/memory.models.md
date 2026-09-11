---
node: mag.runtime.memory.models
---
# mag.runtime.memory.models contract

Embedding, generation, cross-encoding, and late-interaction reranking are
separate model roles behind explicit interfaces. The core quality baseline must
run locally without cloud credentials; remote and self-hosted adapters use the
same memory semantics.

The current production binary constructs the ONNX embedder and may attach the
cross-encoder to the SQLite retrieval path. `memory_core::llm` is feature-gated
infrastructure used by the opt-in, non-persisting `intelligence-produce` CLI
workflow as well as retained experiments and tests. Ordinary ingestion, search,
and MCP do not construct a generation backend. The evaluation command uses
explicit flags over local defaults, not inherited `MAG_LLM_*` configuration.
Its profile describes configured settings, not authenticated model artifacts or
a measured local-model baseline.

Every production model profile declares its model ID, revision, checksums, role,
runtime, quantization, output dimensions, pooling, query/document handling,
maximum input length, licence, and expected local resource envelope. Production
SQLite ingestion, updates, batch writes, semantic search, advanced search, and
query decomposition pass an explicit `EmbeddingInputKind` to the embedding
model boundary. Callers never prepend model-specific text; adapters own query
and document transformation for both single and batched inference. The older
role-neutral `Embedder` interface is supported through one compatibility
adapter rather than remaining the production storage boundary.

Persisted vectors carry an embedding-space identity. A profile change never
silently mixes vector spaces: MAG fails visibly and requires an explicit,
recoverable re-embedding migration. Missing models fail visibly and fall back
only when the caller explicitly permits it. Models with additional licence
conditions are opt-in profiles unless an accepted decision establishes another
default.

## Evaluation generation boundary

The selected runtime calls `LlmBackend::complete` once per request by default.
Explicit schema diagnostics use `complete_constrained` with the fixed runtime
output-shape schema. Neither evaluation mode uses the repairing
structured-completion path. Native constraints retain the same prompt and
decoding configuration; unsupported backends fail without silent fallback. Malformed output remains an attempt
for the independent scorer. The existing HTTP provider trims surrounding
whitespace; this transformation is disclosed in `--describe` metadata. Backend
errors crossing this boundary are redacted, without retries or fallback.

The request and returned completion each have a one-MiB application limit.
The existing HTTP adapter buffers its response before the completion limit is
checked: this is a trusted-endpoint evaluation path, not HTTP memory isolation.
Capture owns the whole-process deadline and stream quotas. No artifact revision,
checksum, quantization, licence, token count, load time, or peak RAM is inferred
from an endpoint's model name. Configured profiles do not satisfy the production
model-verification contract above.
