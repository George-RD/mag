---
node: mag.quality.benchmarks
status: blocked
created: 2026-07-29
---
# Prepare LFM2.5 Specialization Data

Blocked by `todo.calibrate-retrieval-and-reranking`.

Retain versioned, provenance-linked examples from retriever and memory-
intelligence evaluations: query, required evidence, useful candidates, hard
negatives, wrong-time and superseded facts, model profile, scores, feedback, and
downstream task outcome.

After the production retrieval and intelligence algorithms are stable, evaluate
task-specific fine-tuning separately for the LFM2.5 dense retriever, selected
ColBERT reranker, 350M constrained tasks, and 1.2B reasoning tasks. Promotion
requires held-out task-success improvement, reproducible training inputs,
licence metadata, and rollback to the untuned baseline.
