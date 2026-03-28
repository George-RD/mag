# How MAG Works
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

Single binary, single SQLite file, hybrid retrieval.
No external services, no network calls at query time.
Everything runs locally: embeddings (ONNX), full-text search (FTS5), vector KNN, cross-encoder reranking, and a relationship graph -- all inside one process backed by one `memory.db` file (`~/.mag/memory.db`).

## Storage Pipeline

What happens when you call `mag store`:

```
Input text
  |
  v
1. Dedup check (content hash + Jaccard similarity)
  |  duplicate? --> bump access_count, return early
  v
2. Embedding generation (ONNX, ~8ms)
  |
  v
3. Supersession detection (cosine >= 0.70 AND Jaccard >= 0.30)
  |  found older version? --> mark superseded, create SUPERSEDES edge, link version chain
  v
4. INSERT memory + FTS5 index sync
  |
  v
5. Entity extraction (auto-tagging: people, tools, projects)
  |
  v
6. Graph edge creation
   - PRECEDED_BY: link to previous memory in same session (temporal adjacency)
   - RELATES_TO: link to other memories sharing entity tags (co-occurrence)
```

### Dedup (Phase 1)

Two-level dedup runs in a single CTE query:

- **Canonical hash**: SHA-256 of whitespace-normalized, lowercased content. Exact semantic duplicates are caught here with zero compute cost.
- **Jaccard similarity**: For event types that define a dedup threshold, the 5 most recent memories of the same type are fetched and compared using character 3-gram Jaccard similarity. Near-duplicates (e.g. a memory re-stored with minor rephrasing) are caught here.

If either matches, the existing memory's `access_count` is bumped and no new row is inserted. The embedding step is skipped entirely -- this is the cheapest possible path.

### Supersession (Phase 3)

For event types that support versioning (preferences, decisions, etc.), MAG looks for older memories of the same type that are semantically similar:

- **Cosine similarity >= 0.70** (primary signal -- catches semantic overlap even when wording changes)
- **Jaccard word overlap >= 0.30** (secondary signal -- prevents cross-topic false matches)

When both thresholds are met, the old memory is marked `superseded_by_id = <new_id>`, and both are linked into a `version_chain_id`. A `SUPERSEDES` graph edge is also created. Superseded memories are excluded from search results by default.

### Entity Extraction (Phase 5)

A rule-based extractor detects entities in the content and adds them as structured tags:

- **People**: capitalized multi-word names following social cues ("met with Alice", "talked to Bob Smith")
- **Tools/technologies**: matched against a known set (React, Docker, PostgreSQL, etc.)
- **Projects**: backtick-quoted names or CamelCase identifiers

Entities are stored as tags in the format `entity:people:alice`, `entity:tools:react`, `entity:projects:launchpad`. These tags drive entity-based graph edges and entity expansion during search.

## Retrieval Pipeline

MAG exposes three search modes with increasing sophistication.

### Basic Search (`search`)

FTS5 full-text search with BM25 ranking. Fast keyword lookup -- no embeddings involved.

### Semantic Search (`semantic-search`)

Pure vector KNN search via embedding cosine similarity. Good for meaning-based queries where keywords might not match.

### Advanced Search (`advanced-search`)

The primary search mode. A multi-phase pipeline that fuses lexical and semantic signals, enriches results through the graph, and applies a multi-factor scoring model.

```
Query
  |
  v
Phase 0: Intent classification + query embedding (~8ms)
  |          Keyword-only queries skip embedding and vector search entirely.
  |
  +--parallel--+
  |             |
  v             v
Phase 1:     Phase 2:
Vector KNN   FTS5 BM25
(cosine sim) (20x oversample, min 100, max 5000 candidates)
  |             |
  +------+------+
         |
         v
Phase 3: RRF fusion
  |  - Reciprocal Rank Fusion with k=60
  |  - Equal weight: vec=1.0, fts=1.0
  |  - Dual-match boost: candidates in BOTH lists get adaptive 1.3x-1.8x multiplier
  |    (scaled by FTS rank: top FTS match gets 1.8x, lower ranks taper toward 1.3x)
  |
  v
Phase 3b: Cross-encoder reranking (optional)
  |  - ms-marco-MiniLM-L-6-v2 scores top 30 candidates
  |  - Blended: alpha * rrf_score + (1-alpha) * cross_encoder_score (alpha=0.5)
  |
  v
Phase 4: Score refinement (per-candidate multiplicative factors)
  |  - Word overlap boost (stemmed token overlap between query and content+tags)
  |  - Query coverage boost (what fraction of query terms appear in the candidate)
  |  - Jaccard similarity boost (3-gram Jaccard between query and candidate)
  |  - Feedback factor (positive feedback: mild boost up to 1.3x; negative: suppression to 0.1x-0.3x)
  |  - Time decay (1 / (1 + days_old / decay_days); semantic memories exempt)
  |  - Importance factor (floor=0.3 + importance * 0.5)
  |  - Priority factor (base=0.7 + priority * 0.08, priority 1-5)
  |  - Type weight (event-type-specific multiplier)
  |  - Context tag matching (if context_tags provided)
  |
  v
Phase 5: Graph enrichment
  |  - Take top-k seeds (5-8 results)
  |  - Traverse 1-hop neighbors via relationships table (edge weight >= 0.3)
  |  - Neighbor score = seed_score * 0.1 * edge_weight * relation_type_boost
  |    - PRECEDED_BY edges: 1.5x boost (temporal adjacency)
  |    - RELATES_TO / SIMILAR_TO: 1.3x boost (entity-connected)
  |  - Neighbors pass through their own scoring (word overlap, time decay, etc.)
  |  - If neighbor already in results, keep the higher score
  |
  v
Phase 5b: Entity expansion
  |  - Extract entity tags from top-k seed results
  |  - Find other memories with matching entity tags (up to 25)
  |  - Score with entity expansion boost (1.15x), capped at 0.8 * max_seed_score
  |
  v
Phase 6: Abstention + dedup + output
  |  - Content fingerprint dedup (normalized text comparison)
  |  - Abstention gate: if max text_overlap across all candidates < 0.15, return empty
  |    (prevents returning unrelated results when nothing matches)
  |  - Sort by score descending, normalize to 0.0-1.0 range
  |  - Truncate to requested limit
  |
  v
Results (with optional _explain metadata)
```

When the query contains multiple topics, MAG also runs **query decomposition**: it generates sub-queries for each detected topic, runs each through the full pipeline independently, and merges the results.

### Explain Mode

Pass `--explain` to see the score breakdown for each result. The `_explain` metadata object shows:

- `vec_sim`: cosine similarity from vector search
- `fts_rank` / `fts_bm25`: position and BM25 score from FTS
- `rrf_score`: combined RRF score after fusion
- `dual_match`: whether the candidate appeared in both vector and FTS results
- `adaptive_dual_boost`: the actual boost multiplier applied
- `cross_encoder_score`: cross-encoder relevance score (if reranking enabled)
- `word_overlap`, `query_coverage_boost`, `importance_factor`, `feedback_factor`, `time_decay`: per-candidate refinement factors
- `graph_injected`, `graph_seed_id`, `graph_edge_weight`: graph enrichment provenance
- `entity_expansion`, `expanded_from_tag`: entity expansion provenance
- `final_score`: normalized 0.0-1.0 output score

## Graph Model

Memories are connected by typed, weighted edges in a `relationships` table.

| Edge Type | Created By | Meaning |
|---|---|---|
| `PRECEDED_BY` | Auto (at ingest) | Temporal adjacency within the same session. Links each memory to its predecessor. |
| `RELATES_TO` | Auto (at ingest) | Entity co-occurrence. Two memories sharing an `entity:*` tag get linked. |
| `SUPERSEDES` | Auto (at ingest) | Version chain. New memory supersedes an older one of the same type. |
| Custom | `memory_relations add` | User-defined relationships (SIMILAR_TO, SHARES_THEME, etc.). |

Graph edges are bidirectional in queries (both `source_id` and `target_id` are checked) but directional in semantics.

## Embedding Model

- **Default**: `bge-small-en-v1.5` (int8 quantized ONNX, 384 dimensions)
- **Inference**: ~7ms per embedding on CPU, batched via ONNX Runtime with Level3 graph optimization
- **Model size**: ~32 MB on disk (`~/.mag/models/bge-small-en-v1.5-int8/`)
- **Runtime memory**: ~180 MB peak RSS when session is loaded; ONNX session auto-unloads after 10 minutes idle
- **Cache**: LRU cache of 2048 embeddings (SHA-256 keyed) survives session unload, so repeated queries and re-stores are free
- **Tokenizer**: HuggingFace tokenizers, max 512 tokens (truncation, not chunking)
- **Auto-download**: model + tokenizer fetched from HuggingFace on first use

### Cross-Encoder (Optional)

- **Model**: `ms-marco-MiniLM-L-6-v2` (ONNX)
- **Purpose**: reranks top 30 candidates with full query-passage attention (more accurate than embedding similarity)
- **Output**: sigmoid-normalized relevance score per query-passage pair
- **Blending**: `0.5 * rrf_score + 0.5 * cross_encoder_score` (configurable via `rerank_blend_alpha`)
- **Lifecycle**: same lazy-load + 10-minute idle unload pattern as the embedder

## Scoring Parameters

All scoring weights are exposed in `ScoringParams` with sensible defaults tuned on the LoCoMo benchmark:

| Parameter | Default | Purpose |
|---|---|---|
| `rrf_k` | 60.0 | RRF smoothing constant |
| `rrf_weight_vec` | 1.0 | Vector RRF weight |
| `rrf_weight_fts` | 1.0 | FTS RRF weight |
| `dual_match_boost` | 1.5 | Base multiplier for dual-match candidates |
| `word_overlap_weight` | 0.75 | Stemmed word overlap influence |
| `query_coverage_weight` | 0.35 | Query term coverage influence |
| `jaccard_weight` | 0.25 | Jaccard similarity influence |
| `importance_floor` / `_scale` | 0.3 / 0.5 | Importance scoring range |
| `priority_base` / `_scale` | 0.7 / 0.08 | Priority scoring (1-5 scale) |
| `time_decay_days` | 0.0 (off) | Half-life for time decay; 0 = disabled |
| `graph_neighbor_factor` | 0.1 | Graph neighbor score as fraction of seed |
| `graph_min_edge_weight` | 0.3 | Minimum edge weight for graph traversal |
| `preceded_by_boost` | 1.5 | Extra weight for temporal edges |
| `entity_relation_boost` | 1.3 | Extra weight for entity edges |
| `rerank_top_n` | 30 | Candidates sent to cross-encoder |
| `rerank_blend_alpha` | 0.5 | RRF vs cross-encoder blend ratio |
| `abstention_min_text` | 0.15 | Minimum text overlap to return results |
| `feedback_heavy_suppress` | 0.1 | Score multiplier for heavily downvoted memories |
| `feedback_positive_cap` | 1.3 | Max boost from positive feedback |
