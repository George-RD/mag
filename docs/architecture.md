# How MAG Works
<!-- Last verified: 2026-09-04 | Valid for: v0.1.10-dev+ -->

Single binary, single SQLite file, hybrid retrieval.
No external services, no network calls at query time.
Embeddings (ONNX), full-text search (FTS5), vector similarity, cross-encoder reranking and a relationship graph run in one process against one `memory.db` file (`~/.mag/memory.db`).

## Storage Pipeline

What happens when you call `mag store`:

```
Input text
  |
  v
1. Dedup check (content hash + Jaccard similarity)
  |  duplicate? --> bump access_count, return early
  v
2. Embedding generation (ONNX, 7 ms for the default model)
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
   - related: link to the 3 nearest of the 100 most recent memories (cosine >= 0.45)
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

The candidate set is the 10 most recent live memories of the same `event_type`, narrowed to the same `entity_id` when the new memory carries one. An older version outside that window is not found.

When both thresholds are met, the old memory is marked `superseded_by_id = <new_id>`, and both are linked into a `version_chain_id`. A `SUPERSEDES` graph edge is also created. Superseded memories are excluded from search results by default.

### Entity Extraction (Phase 5)

A regex extractor runs after the INSERT and merges what it finds into the row's tags:

- **People**: a capitalized name after "met with", "talked to", "spoke with", "meeting with", "spoke to", "discussed with" or "working with", plus any capitalized word that is not sentence-initial and not in the common-word list.
- **Tools/technologies**: a capitalized token after "use", "using", "deploy", "deployed", "via", "with", "adopting", "adopted", "installed" or "running". There is no list of known tools -- any capitalized word in that position becomes a tool tag.
- **Projects**: backtick-quoted names of 2-30 characters, a name after "project", "initiative", "codebase" or "repo", and CamelCase identifiers.

Entities are stored as tags in the format `entity:people:alice`, `entity:tools:react`, `entity:projects:launchpad`. These tags drive entity-based graph edges and entity expansion during search.

Scored against hand-authored ground truth, this extractor reaches 7.4% micro F1 on the 36-seed memory-intelligence corpus (2026-09-04, bge-small-en-v1.5 int8). See [Measured Results](#measured-results).

## Retrieval Pipeline

MAG exposes three search modes.

### Basic Search (`search`)

FTS5 full-text search with BM25 ranking. Fast keyword lookup -- no embeddings involved.

### Semantic Search (`semantic-search`)

Cosine similarity against every stored embedding. Default builds carry no vector index: the query loads each row that has an embedding and passes the `SearchOptions` filters, scores it in Rust, sorts, and truncates to the limit. Building with `--features sqlite-vec` replaces that scan with a vec0 KNN index; released binaries are built with `real-embeddings,mimalloc` and do not include it, so the scan is what ships.

### Advanced Search (`advanced-search`)

The primary search mode. A multi-phase pipeline that fuses lexical and semantic signals, enriches results through the graph, and applies a multi-factor scoring model.

```
Query
  |
  v
Phase 0: Temporal expansion, intent classification, query embedding (7 ms)
  |          Keyword-intent and empty queries skip embedding and vector search
  |          entirely. Intent also scales the RRF weights and the oversample.
  |
  +--concurrent--+   (two of the four reader connections a file-backed
  |              |    WAL pool opens; an in-memory database has one
  v              v    connection and runs the two scans in sequence)
Phase 1:      Phase 2:
Cosine scan   FTS5 BM25
(newest       (20x oversample, min 100, max 5000 candidates)
 limit*5,
 min 100,
 max 10000
 rows;
 keeps
 sim >= 0.1)
  |             |
  +------+------+
         |
         v
Phase 3: RRF fusion
  |  - Reciprocal Rank Fusion with k=60
  |  - Base weights vec=1.0, fts=1.0, then scaled by intent: a conceptual
  |    query makes them 1.5 and 0.85, a factual one 1.0 and 1.1
  |  - Dual-match boost: candidates in BOTH lists are multiplied by
  |    dual_match_boost + 0.5 / (1 + fts_rank) -- 2.0 at FTS rank 0,
  |    1.75 at rank 1, tending to 1.5 further down the list
  |
  v
Phase 3b: Cross-encoder reranking (`mag serve --cross-encoder` only)
  |  - ms-marco-MiniLM-L-6-v2 scores 30 candidates, taken alternately from
  |    the vector and FTS lists so neither source is truncated away
  |  - Blended: alpha * rrf_score + (1-alpha) * cross_encoder_score (alpha=0.5)
  |
  v
Phase 4: Score refinement (per-candidate multiplicative factors)
  |  - Query coverage boost (1 + overlap^2 * 0.35)
  |  - Word overlap boost (stemmed token overlap between query and
  |    content+tags). When feedback_score is negative, both of these
  |    boosts apply at half strength
  |  - Jaccard similarity boost (3-gram Jaccard between query and candidate)
  |  - Feedback factor (positive: up to 1.3x; negative: 0.3x, or 0.1x at
  |    feedback_score <= -3)
  |  - Time decay (1 / (1 + days_old / time_decay_days)): returns 1.0 for
  |    semantic memories, and for every memory while time_decay_days is 0.0,
  |    which is the default -- decay is off unless you set it
  |  - Importance factor (0.3 + importance * 0.5)
  |  - Context tag matching (if context_tags provided)
  |  Type weight and priority factor are not applied here. Candidates carry
  |  them from Phase 1 and Phase 2, where they set the base score.
  |
  v
Phase 5: Graph enrichment
  |  - Take top-k seeds (k = limit clamped to 5-8)
  |  - Traverse 1-hop neighbors via relationships table (edge weight >= 0.3)
  |  - Neighbor score = seed_score * 0.1 * edge_weight * relation_type_boost
  |    - PRECEDED_BY: 1.5x (temporal adjacency)
  |    - RELATES_TO / SIMILAR_TO / SHARES_THEME / PARALLEL_CONTEXT: 1.3x
  |    - any other edge type, `related` included: no boost
  |  - Neighbors are then scored with their own weights (word overlap 0.5,
  |    importance floor and scale 0.5) plus type weight, priority, time decay
  |    and feedback
  |  - If neighbor already in results, keep the higher score
  |
  v
Phase 5b: Entity expansion
  |  - Take the entity tags of the top `limit` (at most 20) seeds, first 5
  |    distinct tags
  |  - Find other memories carrying those tags (5 per tag, 25 in total)
  |  - Score with entity expansion boost (1.15x), capped at 0.8 * max_seed_score
  |
  v
Phase 6: Dedup + abstention + output
  |  - Content fingerprint dedup (normalized text, highest score per fingerprint)
  |  - Abstention gate, once the query yields at least one token after
  |    stopword filtering: return empty unless some candidate has
  |    text_overlap >= 0.15, or exactly one candidate has cosine >= 0.82
  |    and leads the runner-up by >= 0.035
  |  - Pluggable ScoringStrategy pass (the shipped default returns the score
  |    unchanged)
  |  - Sort by score descending, normalize to 0.0-1.0 against the top score
  |  - Truncate to requested limit
  |
  v
Results (with optional _explain metadata)
```

Phase 1 in that diagram is the default build. With `--features sqlite-vec` it becomes a vec0 KNN over `limit * 10` neighbors (min 200, max 10,000) hydrated by id, and the recency window disappears.

Two caches sit around this pipeline. A 128-entry query cache keyed on (query, limit, options) replays a previous result set for 60 seconds. A 50-memory hot tier, refreshed every 300 seconds, is queried before the candidate scans; when one of its hits carries `_text_overlap` at or above `abstention_min_text`, its rows are merged into the final list.

Query decomposition runs when the query names two or more entities and at least one topic keyword. It builds sub-queries from the first two entities -- each entity alone, and that entity paired with up to three of the topics -- then runs each sub-query through its own embedding, candidate scans, fusion, refinement, enrichment and abstention pass. Sub-query results merge into the base result set by id, the higher score winning, and the merged list is deduped on content fingerprint before truncation. Sub-queries skip cross-encoder reranking.

### Explain Mode

Pass `--explain` to see the score breakdown for each result. The `_explain` metadata object shows:

- `vec_sim`: cosine similarity from vector search
- `fts_rank` / `fts_bm25`: position and BM25 score from FTS
- `rrf_score`: combined RRF score after fusion
- `dual_match`: whether the candidate appeared in both vector and FTS results
- `adaptive_dual_boost` / `fts_text_rel`: the boost multiplier applied and the inverse-rank term it came from
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
| `RELATES_TO` | Auto (at ingest) | Entity co-occurrence. Two memories sharing an `entity:*` tag get linked, up to 3 targets per tag and 5 tags per memory. |
| `related` | Auto (at ingest) | Embedding similarity. The 3 nearest neighbors with cosine >= 0.45, edge weight = that similarity. Without `sqlite-vec` the search covers the 100 most recent memories carrying an embedding, not the whole table. Carries no relation-type boost during graph enrichment. |
| `SUPERSEDES` | Auto (at ingest) | Version chain. Edge runs from the superseded memory to the new one. |
| Custom | `memory_relations add` | User-defined relationships (SIMILAR_TO, SHARES_THEME, etc.). |

Graph edges are bidirectional in queries (both `source_id` and `target_id` are checked) but directional in semantics.

## Embedding Model

- **Default**: `bge-small-en-v1.5` (int8 quantized ONNX, 384 dimensions)
- **Inference**: 7.0 ms per embedding, measured on macOS aarch64 on 2026-03-19; ONNX Runtime builds the session at graph optimization Level3 -- see [Embedding Model Comparison](benchmarks/models.md) for the other models on that run
- **Model size**: ~32 MB on disk (`~/.mag/models/bge-small-en-v1.5-int8/`)
- **Runtime memory**: ~180 MB peak RSS when the session is loaded. Under `mag serve` a 60-second maintenance tick drops the ONNX session once it has been idle for 10 minutes; one-shot CLI commands exit before that matters
- **Cache**: LRU cache of 2048 embeddings (SHA-256 keyed) survives session unload, so repeated queries and re-stores skip inference
- **Tokenizer**: HuggingFace tokenizers, max 512 tokens (truncation, not chunking)
- **Auto-download**: model + tokenizer fetched from HuggingFace on first use

### Cross-Encoder (Optional)

- **Model**: `ms-marco-MiniLM-L-6-v2` (ONNX)
- **Enabled by**: `mag serve --cross-encoder`, which needs the `real-embeddings` feature. No other command loads it
- **Purpose**: reranks 30 candidates with full query-passage attention (more accurate than embedding similarity)
- **Output**: sigmoid-normalized relevance score per query-passage pair
- **Blending**: `0.5 * rrf_score + 0.5 * cross_encoder_score` (configurable via `rerank_blend_alpha`)
- **Lifecycle**: same lazy-load + 10-minute idle unload pattern as the embedder

## Scoring Parameters

Defaults live in `ScoringParams::default()`. The table covers the parameters the pipeline above names; [Configuration](configuration.md#scoring-parameters) lists every field.

| Parameter | Default | Purpose |
|---|---|---|
| `rrf_k` | 60.0 | RRF smoothing constant |
| `rrf_weight_vec` | 1.0 | Vector RRF weight, before intent scaling |
| `rrf_weight_fts` | 1.0 | FTS RRF weight, before intent scaling |
| `dual_match_boost` | 1.5 | Base multiplier for dual-match candidates; the FTS-rank term adds up to 0.5 on top |
| `word_overlap_weight` | 0.75 | Stemmed word overlap influence |
| `query_coverage_weight` | 0.35 | Query term coverage influence |
| `jaccard_weight` | 0.25 | Jaccard similarity influence |
| `context_tag_weight` | 0.25 | Influence of the fraction of `context_tags` a candidate carries |
| `importance_floor` / `_scale` | 0.3 / 0.5 | Importance scoring range |
| `priority_base` / `_scale` | 0.7 / 0.08 | Priority scoring (1-5 scale) |
| `time_decay_days` | 0.0 (off) | Age in days at which the decay factor reaches 0.5; 0 disables decay |
| `graph_neighbor_factor` | 0.1 | Graph neighbor score as fraction of seed |
| `graph_min_edge_weight` | 0.3 | Minimum edge weight for graph traversal |
| `graph_seed_min` / `_max` | 5 / 8 | Bounds on the seed count for graph enrichment |
| `preceded_by_boost` | 1.5 | Extra weight for temporal edges |
| `entity_relation_boost` | 1.3 | Extra weight for entity edges |
| `neighbor_word_overlap_weight` | 0.5 | Word overlap influence for graph-injected neighbors |
| `neighbor_importance_floor` / `_scale` | 0.5 / 0.5 | Importance scoring range for graph-injected neighbors |
| `rerank_top_n` | 30 | Candidates sent to cross-encoder |
| `rerank_blend_alpha` | 0.5 | RRF vs cross-encoder blend ratio |
| `abstention_min_text` | 0.15 | Minimum text overlap to return results |
| `feedback_heavy_threshold` | -3 | Feedback score at or below which heavy suppression applies |
| `feedback_heavy_suppress` | 0.1 | Score multiplier for heavily downvoted memories |
| `feedback_strong_suppress` | 0.3 | Score multiplier for negative feedback above that threshold |
| `feedback_positive_scale` | 0.05 | Per-point positive boost: `1 + score * 0.05` |
| `feedback_positive_cap` | 1.3 | Max boost from positive feedback |

`ENTITY_EXPANSION_BOOST` (1.15) is a compile-time constant, not a `ScoringParams` field.

## Measured Results

The memory-intelligence evaluation scores what this page describes against ground truth authored from the seed text: entity tagging, relative-date retrieval, inferred relationships, TTL expiry, supersession, clustering, provenance links, and whether question retrieval answers or abstains.

```bash
cargo run --release --bin memory_intelligence_eval -- --embedder bge-small
```

The 2026-09-04 run over the 36-seed v1 dataset (sha `3260e0a00beb`, bge-small-en-v1.5 int8) scored entities 7.4% micro F1, temporal 75.0% recall@10, relationships 66.7% recall, lifecycle 100% accuracy, supersession 50.0% F1, grouping 0.0% cluster coverage, provenance 100% link integrity, questions 90.0% recall@10. The unweighted mean over those eight families is 61.1%, a mean over incommensurable metrics. The corpus is 36 seeds, so one case moves a family score by tens of points: `relationships` has three annotated edges, and one missed edge is 33 points. Six further families -- fact extraction, contradiction detection, summarization, relationship typing, entity normalization, `referenced_date` inference -- have no production implementation and score nothing.

Results and history: [Memory Intelligence Results](benchmarks/MEMORY-INTELLIGENCE.md). Method, dataset policy and metric definitions: [Memory Intelligence Evaluation](benchmarks/memory-intelligence.md). Retrieval quality is measured separately by LoCoMo and LongMemEval; see [Benchmark Report](benchmarks/methodology.md).
