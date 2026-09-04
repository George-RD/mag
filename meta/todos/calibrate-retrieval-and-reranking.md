---
node: mag.runtime.memory.retrieval
status: open
created: 2026-07-28
unblocked: 2026-09-04
---
# Calibrate Retrieval And Reranking

Unblocked by `todo.build-local-memory-intelligence-eval-harness` (done),
`todo.define-role-aware-retriever-profiles` (done), and the recoverable
migration slice of `todo.implement-embedding-space-migration`, which remains
`in_progress` only for the profile-selection work this todo does not depend on.

The evaluation gate reports a first baseline and six concrete failures. Start
there rather than with a fresh sweep: abstention returns a false answer on a
query with nothing relevant in the store, a thin query abstains before its date
filter is consulted, and clustering recovers none of the labelled duplicate
groups. See `docs/benchmarks/memory-intelligence.md` and the defect list in
`todo.build-local-memory-intelligence-eval-harness`.

Replace provisional global cutoffs with calibrated confidence from semantic
score, score margin, lexical agreement, reranker score, query intent, and
candidate diversity. Establish the current BGE and MiniLM baseline, then run the
same versioned evaluation across:

- BGE with the existing reranker or no reranker;
- LFM2.5 Embedding as the dense first-stage model;
- BGE with LFM2.5 ColBERT as a bounded top-N reranker;
- LFM2.5 Embedding with LFM2.5 ColBERT.

Include English, Arabic, and English/Arabic cross-lingual cases. Compare
on-demand versus content-hash-cached ColBERT document embeddings. Report
Recall@5/10, MRR, abstention, paraphrase recall, active-injection task success,
cold/warm p50 and p95, RAM, model load time, re-embedding time, database/index
growth, and offline operation.

Keep BGE as the default unless a candidate materially improves MAG-specific task
success within the local footprint and licence budget. Treat a full ColBERT
multi-vector index as a separate decision after bounded reranking identifies
first-stage recall as the remaining bottleneck. Add dynamic result count and
token budget only from the same calibrated evidence.
