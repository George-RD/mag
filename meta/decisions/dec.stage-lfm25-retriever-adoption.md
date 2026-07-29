---
id: dec.stage-lfm25-retriever-adoption
nodes:
  - mag.runtime.memory.models
  - mag.runtime.memory.retrieval
  - mag.runtime.memory.storage.sqlite
  - mag.quality.benchmarks
status: accepted
date: 2026-07-29
revisit_triggers:
  - "MAG evaluation shows material task-success gains within local budgets"
  - "Bounded ColBERT reranking leaves first-stage recall as the bottleneck"
  - "The LFM licence or supported deployment formats change"
  - "A stronger permissively licensed edge retriever becomes available"
informed_by: [res.lfm25-retriever-fit]
related: [dec.lfm25-1-2b-baseline, dec.local-first-dual-mode]
---
# Stage LFM2.5 retriever adoption behind evidence

MAG will evaluate both LFM2.5 retrievers without changing the production default
in advance of evidence.

1. `LFM2.5-Embedding-350M` is a candidate optional multilingual and
   cross-lingual first-stage profile. BGE remains the compact default until the
   candidate passes MAG-specific quality, latency, memory, disk, migration, and
   licence gates.
2. `LFM2.5-ColBERT-350M` will first be evaluated as a bounded top-N reranker
   behind the existing reranker interface. A full multi-vector index requires a
   later decision supported by measured first-stage recall limits.
3. Model profiles, query/document role semantics, embedding-space identity,
   explicit re-embedding, and per-profile calibration are prerequisites for
   production adoption.
4. Evaluation runs should retain training examples and hard negatives, but
   fine-tuning begins only after the selected retrieval architecture and
   held-out task-success measures are stable.

This keeps the immediate experiment reversible, preserves MAG's small local
baseline, and leaves room for a higher-quality multilingual tier where its
larger footprint is justified.
