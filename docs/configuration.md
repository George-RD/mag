# Configuration & Tuning

## Data Location

All MAG data lives in `~/.mag/`:

| Path | Description |
|------|-------------|
| `memory.db` | SQLite database (memories, relationships, embeddings) |
| `models/` | ONNX embedding models (~32 MB for default model) |
| `benchmarks/` | Benchmark dataset cache |
| `backups/` | Binary backups (managed via `memory_lifecycle` action=backup) |

## Embedding Models

Default: `bge-small-en-v1.5` (int8, 384-dim, ~32 MB on disk, ~180 MB RAM)

Models auto-download on first use and are cached under `~/.mag/models/`. Available models can be selected via benchmark flags. See [Model Comparison](benchmarks/models.md) for the full table.

## Scoring Parameters

These parameters control how MAG ranks search results in the advanced multi-phase retrieval pipeline. They are defined in `ScoringParams` and can be tuned for different use cases.

### RRF (Reciprocal Rank Fusion)

The first phase fuses vector similarity and FTS5 BM25 results using Reciprocal Rank Fusion.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `rrf_k` | 60.0 | RRF ranking constant. Higher values flatten rank differences. |
| `rrf_weight_vec` | 1.0 | Weight for vector similarity results in RRF fusion |
| `rrf_weight_fts` | 1.0 | Weight for FTS5 BM25 results in RRF fusion |
| `dual_match_boost` | 1.5 | Fallback multiplier for candidates appearing in both vector and FTS result lists. The pipeline uses an adaptive boost (1.3x--1.8x) scaled by FTS rank; this value is the fallback when rank is unavailable. |

### Reranking

Optional cross-encoder reranking of the top candidates after RRF fusion.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `rerank_top_n` | 30 | Number of top candidates to rerank with the cross-encoder |
| `rerank_blend_alpha` | 0.5 | Blend weight: `alpha * rrf_score + (1 - alpha) * cross_encoder_score`. 0.5 gives equal weight to both. |

### Text Overlap & Coverage

Word-level matching signals used during score refinement.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `word_overlap_weight` | 0.75 | Weight for word overlap between query and result content |
| `query_coverage_weight` | 0.35 | Weight for query term coverage (fraction of query terms found) |
| `jaccard_weight` | 0.25 | Weight for Jaccard similarity between query and result tokens |

### Importance & Priority

How stored importance and priority values affect ranking.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `importance_floor` | 0.3 | Minimum importance factor (memories with importance=0 still get this floor) |
| `importance_scale` | 0.5 | Scaling factor for importance contribution |
| `priority_base` | 0.7 | Base priority factor |
| `priority_scale` | 0.08 | Per-unit priority scaling. Factor = `priority_base + priority * priority_scale` |

### Context Tags

Boost for memories whose tags match the current search context.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `context_tag_weight` | 0.25 | Additive boost per matching context tag |

### Time Decay

Optional recency bias for episodic (non-semantic) memories. Disabled by default.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `time_decay_days` | 0.0 | Half-life in days. Formula: `1 / (1 + age_days / time_decay_days)`. Set to 0 to disable (default). Semantic memories (decisions, lessons, preferences) are never decayed. |

### Feedback

How user feedback (helpful/unhelpful/outdated) affects future ranking. Asymmetric by design: negative feedback suppresses aggressively, positive feedback gives only mild boosts to avoid displacing unrelated results.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `feedback_heavy_threshold` | -3 | Score at or below this triggers heavy suppression |
| `feedback_heavy_suppress` | 0.1 | Multiplier for heavily downvoted memories (near-total suppression) |
| `feedback_strong_suppress` | 0.3 | Multiplier for negative feedback (strong suppression) |
| `feedback_positive_scale` | 0.05 | Per-point positive boost: `1 + score * 0.05` |
| `feedback_positive_cap` | 1.3 | Maximum positive feedback multiplier |

### Graph Enrichment

Phase 5 of the pipeline enriches results with graph-connected memories.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `graph_neighbor_factor` | 0.1 | Score fraction assigned to graph neighbors. Neighbors get at most 10% of the seed score. Set to 0.0 to disable graph enrichment. |
| `graph_min_edge_weight` | 0.3 | Minimum edge weight to traverse during enrichment |
| `graph_seed_min` | 5 | Minimum number of seed results before graph enrichment activates |
| `graph_seed_max` | 8 | Maximum number of seed results to use for graph expansion |
| `preceded_by_boost` | 1.5 | Multiplicative boost for PRECEDED_BY edges (temporal adjacency). Adjacent conversation turns get 50% more weight. |
| `entity_relation_boost` | 1.3 | Multiplicative boost for entity-related edges (RELATES_TO, SIMILAR_TO). Entity-connected memories get 30% more weight. |
| `neighbor_word_overlap_weight` | 0.5 | Word overlap weight for scoring graph neighbors (vs seed query) |
| `neighbor_importance_floor` | 0.5 | Importance floor for graph neighbors |
| `neighbor_importance_scale` | 0.5 | Importance scale for graph neighbors |

### Abstention Gate

Phase 6: filters out low-relevance results to avoid returning noise.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `abstention_min_text` | 0.15 | Minimum text overlap score for a result to pass the gate. Results below this threshold are dropped. Lowered from 0.30 to handle numeric/synonym-heavy queries. |

### Constants (not in ScoringParams)

These are compile-time constants, not tunable at runtime.

| Constant | Value | Description |
|----------|-------|-------------|
| `ENTITY_EXPANSION_BOOST` | 1.15 | Multiplicative boost for memories found via entity tag expansion |

## Tuning Guide

### Precision over recall

Increase the thresholds to return fewer but more relevant results:

- Raise `abstention_min_text` (e.g., 0.25) to drop marginal results
- Raise `importance_floor` (e.g., 0.5) to prefer high-importance memories
- Lower `graph_neighbor_factor` (e.g., 0.05 or 0.0) to reduce graph noise

### Recency bias

For use cases where recent memories should rank higher:

- Set `time_decay_days` to a positive value (e.g., 30 = half-life of one month)
- Note: semantic memory types (decisions, lessons, preferences) are exempt from time decay

### Graph-heavy workloads

When memories form rich relationship graphs (e.g., conversation threads, knowledge graphs):

- Increase `graph_neighbor_factor` (e.g., 0.2--0.3)
- Increase `preceded_by_boost` (e.g., 2.0) for conversation-heavy data
- Increase `entity_relation_boost` (e.g., 1.5) for entity-rich data
- Increase `graph_seed_max` (e.g., 12) to explore more graph paths

### Keyword-heavy queries

When users primarily search with specific keywords:

- Increase `word_overlap_weight` (e.g., 1.0)
- Increase `query_coverage_weight` (e.g., 0.5)
- Consider increasing `rrf_weight_fts` relative to `rrf_weight_vec`

### Semantic-heavy queries

When users primarily search with conceptual/natural language queries:

- Increase `rrf_weight_vec` relative to `rrf_weight_fts`
- Lower `word_overlap_weight` (e.g., 0.4) since semantic matches may use different words
- Lower `abstention_min_text` (e.g., 0.10) to allow semantic-only matches through

### Feedback-driven curation

When you want aggressive feedback effects:

- Lower `feedback_strong_suppress` (e.g., 0.1) for stronger negative signal
- Raise `feedback_positive_cap` (e.g., 1.5) for stronger positive signal
- Lower `feedback_heavy_threshold` (e.g., -5) to require more downvotes before heavy suppression

## Search Pipeline Overview

For reference, the advanced search pipeline runs these phases in order:

1. **Intent classification** -- Categorize query as Keyword, Factual, Conceptual, or General
2. **Retrieval** -- Vector similarity search + FTS5 BM25 (skip vector for Keyword intent)
3. **RRF fusion** -- Merge results with reciprocal rank fusion + dual-match boost
4. **Cross-encoder rerank** -- Rerank top-N candidates (optional)
5. **Score refinement** -- Apply type weight, time decay, priority, word overlap, importance, feedback, query coverage
6. **Graph enrichment** -- Expand results with graph-connected neighbors (when factor > 0)
7. **Abstention gate** -- Drop results below the text overlap threshold
