# AutoMem — Search & Recall Pipeline

Primary file: `automem/api/recall.py` (~1750 lines)
Supporting: `automem/search/runtime_recall_helpers.py`, `runtime_keywords.py`, `runtime_relations.py`, `automem/utils/scoring.py`

## Three Search Methods

| Method | Store | How |
|--------|-------|-----|
| Vector search | Qdrant | Embedding cosine similarity |
| Graph keyword search | FalkorDB | Exact/phrase match on memory content |
| Tag-only fallback | FalkorDB | Pure tag filter, no query |

Vector and graph keyword are run together for each sub-query; tag-only is a fallback when both return nothing.

## Query Decomposition

For multi-hop reasoning, the query is auto-decomposed into entity + topic sub-queries before search:

```
"Would Caroline pursue writing?"
→ ["Caroline", "Caroline writing", "Caroline interests", "writing"]
```

Decomposition extracts named entities and generates compound queries. Each sub-query runs the full search + scoring cycle independently. Results are merged with deduplication.

## Recall Pipeline (step by step)

1. **Parse parameters** — query, tags, time filters, language, context (priority tags/IDs), sort order
2. **Auto-decompose** — optional, generates sub-queries
3. **Per sub-query:**
   a. Vector search (Qdrant, top-N by cosine)
   b. Graph keyword search (FalkorDB, exact/phrase)
   c. Tag-only fallback if (a) and (b) both empty
4. **Priority injection** — if context specifies anchor IDs/tags, force those memories into candidates
5. **Metadata scoring** — 10-component weighted sum (see below)
6. **Relation expansion** — graph traversal from top-N results; boost = 0.25 × seed score
7. **Entity expansion** — search for memories about extracted entities
8. **Deduplication** — by ID, then by content fingerprint; keep highest-scoring duplicate
9. **Sort** — by score (default), timestamp, or updated_at
10. **Return** with debug metadata (per-result score components, source, dedup info)

## Scoring Formula (10 Components)

All weights controlled via `SEARCH_WEIGHT_*` environment variables:

| Component | Default Weight | Notes |
|-----------|---------------|-------|
| Vector similarity | 0.35 | Cosine from Qdrant |
| Keyword match | 0.35 | FalkorDB keyword hit |
| Tag match | 0.20 | Exact tag overlap |
| Exact phrase | 0.20 | Substring/phrase in content |
| Graph relationships | 0.25 | Edge strength from traversal |
| Importance | 0.10 | Memory importance score (0–1) |
| Confidence | 0.05 | Memory confidence score (0–1) |
| Recency | 0.10 | Linear decay over 180 days |
| Relevance | 0.00 | Stored relevance score (unused in scoring) |
| Context bonus | — | Stacks on top (see below) |

Context bonuses (from `scoring.py`) stack and can push total above 1.0:
- Tag hit in priority context: **+0.45**
- Memory type match: **+0.25**
- Anchor ID match: **+0.90**

## Keyword Extraction (`runtime_keywords.py`)

Simple tokenizer: split on non-alphanumeric, filter stopwords, keep tokens ≥ 3 chars. No stemming or lemmatization.

## Relation Expansion (`runtime_relations.py`)

Cypher graph traversal with configurable depth (1–3 hops). For each top-N seed result:
- Finds related memories via graph edges
- Extracts edge strength from properties in priority order: `strength`, `score`, `confidence`, `similarity`
- Expanded result score = seed_score × 0.25 + edge_strength (weighted)

Relation types are filtered — only meaningful semantic relationships traversed (not all edge types).

## Multi-hop Recall (`multi_hop_recall_with_graph`)

Separate code path for multi-hop queries (LoCoMo cat 3):
- `initial_limit=20` — seeds from vector search
- `max_connected=60` — max graph-traversal results
- Uses graph BFS from seeds, collects connected memories
- Returns merged set with embedding similarity threshold ≥ 0.50

## Context-Aware Features

- **Language detection** — from query/path/context, boosts memories in matching language style
- **Priority profiles** — let caller anchor results to specific tags/types/IDs (useful for per-conversation isolation)
- **Metadata search** — can index and search the `metadata` dict fields, not just content
- **Time filters** — ISO date range filtering applied at FalkorDB query level

## Debug Output

Each result carries score component breakdown: which vector score, keyword score, relation score, tag bonus, etc. contributed to the final score. Enabled when `debug=true` in request.
