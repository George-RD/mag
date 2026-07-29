---
id: res.lfm25-retriever-fit
nodes:
  - mag.runtime.memory.models
  - mag.runtime.memory.retrieval
  - mag.runtime.memory.storage.sqlite
  - mag.quality.benchmarks
sources: [src.lfm25-retriever-official-materials, src.local-first-roadmap]
date: 2026-07-29
---
# LFM2.5 retriever fit for MAG

MAG already separates dense embedding and reranking roles. The current production
path uses a 384-dimensional BGE embedding and can optionally rerank a bounded
candidate set through the `Reranker` trait. The new LFM2.5 retrievers fit those
two roles differently.

## Dense retriever

`LFM2.5-Embedding-350M` is a multilingual dense bi-encoder. It emits one
1024-dimensional CLS-pooled vector per query or document and uses asymmetric
`query:` and `document:` prompts. Its strongest MAG use is an optional
multilingual and cross-lingual first-stage retrieval profile, including
English/Arabic memory.

It is not a model-file swap for the current embedder:

- query and document encoding must be role-aware;
- the pooling contract differs from MAG's current mean-pooling path;
- the vector dimension grows from 384 to 1024;
- stored memories must be re-embedded into the new vector space;
- similarity, margin, supersession, fusion, and abstention thresholds must be
  calibrated for the new score distribution.

The raw f32 vector payload is about 2.67 times the current one before database
and index overhead. It therefore needs an explicit quality-versus-footprint
decision rather than automatic replacement of BGE.

## Late-interaction retriever

`LFM2.5-ColBERT-350M` emits one 128-dimensional vector per token and scores
query/document pairs with MaxSim. MAG should evaluate it first as a bounded
top-N reranker behind the existing `Reranker` boundary. This preserves the
current dense/FTS candidate generation and avoids committing to a new
multi-vector primary index.

The first experiment should compare:

- encoding candidate passages on demand;
- caching document token embeddings by content hash;
- the existing MiniLM cross-encoder;
- no reranker.

A full PLAID-style ColBERT index is a separate architectural decision. It should
be considered only if bounded reranking materially improves downstream task
success and the remaining bottleneck is first-stage recall rather than ranking.

## Required foundation

Production qualification requires:

1. a model profile that records model ID, revision, checksum, role, runtime,
   quantization, dimensions, pooling, prefixes, maximum length, and licence;
2. role-aware query/document embedding without model-specific strings leaking
   through ingestion and search call sites;
3. persisted embedding-space identity plus transactional batch re-embedding;
4. per-profile calibration rather than shared global thresholds;
5. runtime adapters behind the model/runtime boundary, without assuming that the
   generative-model runtime is also the best retriever runtime;
6. explicit licence handling so a restricted model is not silently made the
   universal default.

## Evaluation matrix

The same versioned corpus and query set should compare:

| Profile | First-stage dense model | Reranker |
|---|---|---|
| Current baseline | bge-small-en-v1.5 | MiniLM or none |
| Multilingual candidate | LFM2.5 Embedding | MiniLM or none |
| Late-interaction value | bge-small-en-v1.5 | LFM2.5 ColBERT |
| Full LFM retrieval | LFM2.5 Embedding | LFM2.5 ColBERT |

The scorecard should include Recall@5/10, MRR, abstention accuracy, paraphrase
recall, active-injection task success, English retrieval, Arabic retrieval,
English/Arabic cross-lingual retrieval, p50/p95 cold and warm latency, RAM, model
load time, re-embedding time, database/index growth, and offline success.

## Fine-tuning sequence

MAG should retain evaluated query-to-memory examples while the retrieval
architecture is calibrated:

- required evidence memories and useful candidates;
- same-project and same-entity hard negatives;
- correct entity but wrong time;
- superseded versus current facts;
- lexical near-matches that are semantically wrong;
- downstream task success and user feedback.

Training should wait until candidate generation, ranking semantics, abstention,
and evaluation labels are stable. The dense retriever is the first likely
specialization target. ColBERT should be fine-tuned only if its reranker role is
selected. Generative LFM2.5 models remain separate task-specific candidates for
classification, extraction, relationships, and consolidation.

## Conclusion

Keep BGE as the compact permissively licensed default until MAG-specific
evidence supports a change. Evaluate LFM2.5 Embedding as an optional multilingual
profile and LFM2.5 ColBERT first as an optional quality reranker. Collect
fine-tuning data during evaluation, but promote no fine-tuned model before the
retrieval architecture and held-out task-success gates are stable.
