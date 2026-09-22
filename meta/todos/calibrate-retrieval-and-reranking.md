---
node: mag.runtime.memory.retrieval
status: in_progress
created: 2026-07-28
---
# Calibrate Retrieval And Reranking

Unblocked: the local evaluation harness, role-aware retriever profiles and
embedding-space migration todos are done on main. Recovery of the historical
runtime-behaviour diagnostic supplies additional calibration inputs; it does
not complete the retrieval matrix or justify changed thresholds.

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

## Selective recovery

PR #451 recovers the Claude branch's eight-family stored-memory diagnostic
under `benches/runtime_behaviour/`, separate from the current generation scorer.
Preserve the original dataset, use the shared BGE adapter, expose missing
measurements, and re-run rather than importing old scores. See
`meta/reviews/runtime-behaviour-recovery.md`. The original branch remains until
all useful work has been accounted for. Next work is measured retrieval/model
calibration and held-out coverage, not tuning this seed to improve its score.
