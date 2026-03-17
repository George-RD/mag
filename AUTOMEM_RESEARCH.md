# AutoMem Search/Recall System Research Summary

## Overview
AutoMem is a **hybrid vector-graph-keyword memory system** with sophisticated context-aware retrieval. The recall system orchestrates multiple search strategies (vector, keyword, graph, relations) and scores results using a weighted combination of relevance signals.

---

## Architecture: Core Components

### 1. **runtime_recall_helpers.py** - Search Orchestration Layer
Provides low-level helpers that coordinate between Qdrant (vector store) and FalkorDB (graph store).

**Key Functions:**
- `_result_passes_filters()` - Applies time-range, tag, and exclusion filters
- `_format_graph_result()` - Wraps graph nodes into result dicts with relations
- `_graph_trending_results()` - Retrieves top memories by importance (no query)
- `_graph_keyword_search()` - Full-text keyword search on graph content
- `_vector_search()` - Semantic vector search via Qdrant
- `_vector_filter_only_tag_search()` - Pure tag-based filtering (no query)

**Search Flow (Priority Order):**
1. Vector search (semantic, embedding-based)
2. Graph keyword search (exact/phrase matching in graph)
3. Tag-only search (fallback when no query)
4. Trending (fallback when results are sparse)

**Key Insight:** Results include `relations` array populated by `fetch_relations()` - each result knows its graph neighbors.

---

### 2. **runtime_keywords.py** - Keyword Extraction
Simple stateless keyword extraction for search queries.

**Key Functions:**
- `load_keyword_runtime()` - Loads stopwords and entity blocklists
- `_extract_keywords()` - Extracts 3+ char tokens, filters stopwords

**Algorithm:**
- Tokenize on `[A-Za-z0-9_\-]`
- Filter out stopwords (common English words)
- Deduplicate, return ordered list

**Note:** Also imports from `automem.utils.text` if available for enhanced extraction.

---

### 3. **runtime_relations.py** - Graph Traversal & Relationship Fetching
Implements graph relationship queries with multi-hop support.

**Key Functions:**
- `fetch_relations()` - Gets all outgoing edges from a memory node
  - Returns: list of `{type, strength, kind, memory}` objects
  - Sorts by `r.updated_at` or `related.timestamp` (newest first)
  - Extracts strength from multiple edge properties: `r.strength`, `r.score`, `r.confidence`, etc.

- `get_related_memories()` - REST endpoint for relationship traversal
  - Supports: `max_depth` (1-3), `relationship_types` filter, `limit`
  - Executes Cypher query with pattern matching
  - Returns distinct related memories with importance/timestamp ordering

**Cypher Pattern:**
```cypher
MATCH (m:Memory {id: $id})-[r:TYPE1|TYPE2|...*1..depth]->(related:Memory)
WHERE m.id <> related.id
RETURN DISTINCT related
ORDER BY importance DESC, timestamp DESC
```

---

### 4. **recall.py** - Main Recall Pipeline (Largest File, ~1750 lines)
Orchestrates the full recall system with context-aware scoring and multi-hop expansion.

#### **Sub-Functions: Query Analysis**
- `_extract_query_entities()` - Extracts capitalized names from queries
  - Detects patterns like "Caroline's" → "Caroline"
  - Filters stopwords, skips sentence-initial caps
  - Purpose: Enable entity-specific follow-up searches

- `_extract_topic_keywords()` - Extracts meaningful topics from queries
  - Filters stop words, limits to 5 topics
  - Purpose: Broaden search beyond entity names

- `_fingerprint_content()` - Lightweight dedup key (~320 chars)
  - Strips markdown, punctuation, normalizes whitespace
  - Used in result deduplication

- `_dedupe_results()` - Removes duplicate memories
  - Buckets by ID first, then by content fingerprint
  - Keeps highest-score/newest when duplicates exist
  - Returns deduped list + count of removed items

#### **Sub-Functions: Context & Priority**
- `_detect_language_hint()` - Infers programming language from query/path/context
  - Checks explicit param, context label, file extension, query tokens
  - Maps `.py` → `python`, `.ts` → `typescript`, etc.
  - Returns canonical language name

- `_build_context_profile()` - Creates a scoring context object
  - Inputs: priority tags, priority memory IDs, language hint, context label
  - Outputs: `{priority_tags, priority_types, priority_ids, priority_keywords, weights}`
  - Adds style/preference tags if language-focused
  - Used downstream in `_result_matches_context_priority()` and `_compute_context_bonus()`

- `_result_matches_context_priority()` - Boolean: does result match priority context?
  - Checks tags, types, keywords, IDs against priority profile

- `_inject_priority_memories()` - Ensures high-priority results appear in final set
  - If context profile specifies priority IDs but they're missing from results:
    - Runs targeted queries (graph keyword + tag filter searches)
    - Adds them to result list
  - Similar for priority tags/types (searches for matching results)

- `_guarantee_priority_results()` - Reorders final results
  - If context-priority results exist, move them to top N slots
  - Maintains score-based ordering within each tier

#### **Sub-Functions: Relation Expansion**
- `_extract_entities_from_results()` - Collects named entities from matched memories
  - Looks for `entity` field in memory metadata
  - Purpose: Find related memories about the same people/places

- `_expand_entity_memories()` - Finds memories about extracted entities
  - For each entity: searches memory content for that entity name
  - Runs graph keyword search on entity name
  - Returns expanded memories with `match_type: "entity"`

- `_expand_related_memories()` - Graph-based relation expansion
  - For each seed result, traverses outgoing edges (all relation types or filtered)
  - Applies filters: `expand_min_strength`, `expand_min_importance`
  - Scores expanded results: `relation_score = strength + seed_score * boost`
  - Combines multiple paths to same memory (max relation_score wins)
  - Re-scores with `compute_metadata_score()` (includes relation component)
  - Returns up to `expansion_limit` results

#### **Main Function: `handle_recall()`**
The orchestrator. Signature shows complexity:
```python
def handle_recall(
    get_memory_graph, get_qdrant_client,
    normalize_tag_list, normalize_timestamp, parse_time_expression,
    extract_keywords, compute_metadata_score,
    result_passes_filters,
    graph_keyword_search, vector_search, vector_filter_only_tag_search,
    recall_max_limit, logger,
    allowed_relations=None, default_expand_relations=None,
    relation_limit=None, expansion_limit_default=None,
    on_access=None, jit_enrich_fn=None,
):
```

**Core Logic:**
1. **Parse Query Parameters**
   - `query` / `queries` (multi-query support)
   - Time filters: `start`, `end`, `time_query`
   - Tag filters: `tags`, `tag_mode` (any/all), `tag_match` (prefix/exact)
   - Exclusions: `exclude_tags`
   - Sorting: `sort` (score/time_desc/time_asc/updated_desc/updated_asc)
   - Language hint, active file path, context label

2. **Auto-Decomposition** (optional)
   - Extract entities + topics from query
   - Generate supplementary queries:
     - Entity alone (implicit context)
     - Entity + each topic
     - Entity + broad terms (interests, goals, plans)
     - Topic-only queries
   - Run recall separately on each decomposed query, merge results

3. **Per-Query Recall** (`_run_single_query`)
   - Build language context profile
   - Vector search (if Qdrant available)
   - Graph keyword search (fills remaining slots)
   - Tag-only search (pure filter fallback)
   - Inject priority memories if missing
   - Compute metadata scores
   - Apply filters (time, tags, min_score)
   - Sort by score/time
   - Reorder for priority
   - Return results + context metadata

4. **Multi-Query Merging**
   - Combine results from all queries
   - Deduplicate by ID/fingerprint
   - Re-sort globally

5. **Post-Processing Expansion** (optional)
   - Entity expansion: find memories about extracted entities
   - Relation expansion: traverse graph from seed results
   - JIT enrichment: optional runtime enrichment function

6. **Final Response**
   - Filter by min_score (adaptive or fixed)
   - Cap at recall_limit
   - Return with debug metadata:
     - `score_components` (vector, keyword, relation, tag, importance, confidence, recency, exact, relevance, context)
     - `final_score` (weighted sum)
     - `original_score` (pre-rescoring)
     - `_query` (originating query in multi-query)
     - Dedup metadata if applicable

---

### 5. **runtime_recall_routes.py** - Route Handler Wrapper
Thin wrapper that calls `handle_recall()` and emits telemetry.

**Key Function:**
- `recall_memories()` - Flask route handler
  - Parses query params
  - Calls `handle_recall_fn()`
  - Emits `memory.recall` event (query, limit, result_count, elapsed_ms, tags)
  - Returns JSON response

---

### 6. **scoring.py** - Multi-Component Scoring System
Implements the final `_compute_metadata_score()` that powers result ranking.

**Scoring Components:**

| Component | Weight Env Var | Computed From |
|-----------|----------------|--------------|
| `vector` | `SEARCH_WEIGHT_VECTOR` | Hit's vector similarity score |
| `keyword` | `SEARCH_WEIGHT_KEYWORD` | Graph keyword match score |
| `relation` | `SEARCH_WEIGHT_RELATION` | Relation expansion strength |
| `tag` | `SEARCH_WEIGHT_TAG` | Tag matches / token count |
| `importance` | `SEARCH_WEIGHT_IMPORTANCE` | Memory's `importance` field (0-1) |
| `confidence` | `SEARCH_WEIGHT_CONFIDENCE` | Memory's `confidence` field (0-1) |
| `recency` | `SEARCH_WEIGHT_RECENCY` | Decay function over 180 days |
| `exact` | `SEARCH_WEIGHT_EXACT` | 1.0 if query exact-matches metadata |
| `relevance` | `SEARCH_WEIGHT_RELEVANCE` | Consolidation decay score (optional) |
| **Context** | Custom | Tag/type/keyword/anchor bonuses |

**Recency Score:**
```
age_days = (now - timestamp).days
recency = max(0, 1 - age_days / 180)  # Linear decay over 6 months
```

**Context Bonus (from context_profile):**
- Tag hit: +0.45
- Type match: +0.25
- Keyword match: +0.2
- Anchor ID match: +0.9
- Can stack (up to 1.8 max typical)

**Metadata Scoring:**
- Collects metadata dict from memory
- Extracts all string values + tokens (text search terms)
- Counts matching tokens (tag_score = hits / max(tokens, 1))
- Evaluates exact phrase match

**Final Score:**
```
final = sum(weight[i] * component[i] for i in components) + context_bonus
```

All weights are configurable via environment variables.

---

## Key Insights: How Recall Actually Works

### **Search Strategy Hierarchy**
1. **Vector (semantic)**: Embedding-based, catches paraphrases
2. **Keyword (graph FTS)**: Exact/phrase matches, fast local search
3. **Tag-only (filter)**: When no query but tags specified
4. **Trending**: Fallback, returns by importance

Results from all combine in `local_results` up to per_query_limit.

### **Context-Aware Scoring**
- **Language detection**: Query → Python style memory boost
- **Priority injection**: Context profile → force high-priority results in
- **Tag/type bonuses**: Coding-style tag → +0.45 if query semantic
- **Metadata search**: Token matching in memory metadata (not just content)

### **Multi-Hop Expansion**
- **Entity expansion**: Extract names from query, find memories about them
- **Relation expansion**: For top N results, traverse graph neighbors
  - Respects `expand_min_strength` filter
  - Respects `expand_min_importance` filter
  - Boosts expanded results by seed score (`relation_score = strength + seed * 0.25`)
  - Deduplicates: same memory from multiple paths → max relation_score wins
  - Caps at `expansion_limit` (default 500)

### **Query Decomposition**
- "Would Caroline pursue writing?" decomposes into:
  - "Caroline" (entity)
  - "Caroline writing" (entity + topic)
  - "Caroline interests goals plans" (entity + broad context)
  - "writing" (topic alone)
- Each query scored independently, results merged with global deduplication

### **Result Deduplication**
- Primary key: memory ID
- Fallback: content fingerprint (320 chars, normalized)
- When duplicate found:
  - Keep the one with highest final_score
  - If tied, keep the newest by timestamp
  - Track sources for debugging

### **Sorting & Presentation**
- **score**: By final_score (default), with tie-breaks (source, original_score, importance)
- **time_desc/time_asc**: By timestamp DESC/ASC, then by updated_at, then by ID
- **Priority reordering**: Context-priority results moved to top N slots

---

## Configuration & Tuning

**Key Environment Variables:**
- `RECALL_MIN_SCORE`: Minimum final_score threshold (default: None)
- `RECALL_ADAPTIVE_FLOOR`: Auto-threshold based on top result
- `RECALL_EXPANSION_LIMIT`: Max related memories to fetch (default 500)
- `RECALL_RELATION_LIMIT`: Max relations per seed (default 5)
- `DEFAULT_EXPAND_RELATIONS`: Which relation types to expand by default
- `SEARCH_WEIGHT_*`: Individual component weights
- `COLLECTION_NAME`: Qdrant collection (default "memories")

**Request Parameters:**
- `expand_relations`: Boolean, enable relation expansion
- `expand_entities`: Boolean, enable entity expansion
- `expand_min_strength`: Threshold for relation edge strength (0-1)
- `expand_min_importance`: Threshold for expanded memory importance (0-1)
- `auto_decompose`: Boolean, enable query decomposition
- `language`: Explicit language hint (overrides detection)
- `context_tags`: Priority tags (boost matching results)
- `context_types`: Priority memory types
- `priority_ids`: Ensure these memories appear in results

---

## Integration Points

**Callable Dependencies Injected into `handle_recall()`:**
- `get_memory_graph()` → FalkorDB connection
- `get_qdrant_client()` → Qdrant connection
- `extract_keywords(text)` → Keyword extraction
- `compute_metadata_score(result, query, tokens, context_profile)` → Final scoring
- `result_passes_filters(result, start, end, tags, mode, match, exclude)` → Filtering
- `graph_keyword_search(...)` → FTS on graph
- `vector_search(...)` → Semantic search
- `jit_enrich_fn()` → Optional runtime enrichment
- `on_access()` → Optional access tracking

**Why DI?** Enables testing, swapping implementations (mock graph, in-memory Qdrant).

---

## Summary: The Full Recall Loop

```
User Query
  ↓
Parse params (query, tags, language, context, limits)
  ↓
Auto-decompose query (optional) → multiple sub-queries
  ↓
For each query:
  ├─ Detect language context
  ├─ Build priority profile (tags, types, IDs)
  ├─ Vector search (semantic)
  ├─ Graph keyword search (exact match)
  ├─ Tag-only search (fallback)
  ├─ Inject priority results (if missing)
  └─ Score & filter locally
  ↓
Merge all query results + deduplicate
  ↓
Expand related memories (graph traversal, optional)
  ↓
Expand entity memories (graph keyword on entities, optional)
  ↓
Final scoring:
  ├─ Re-score all with metadata
  ├─ Apply min_score threshold
  ├─ Filter by time/tags/exclusions
  └─ Sort (score, time, or updated)
  ↓
Reorder for context priority
  ↓
Return with debug metadata (score components, dedup info, source)
```

---

## Performance Characteristics

**Typical Query Cost:**
- Vector search: O(log n) with Qdrant indexing (~10-50ms for 1M memories)
- Graph keyword search: O(n) scan + FTS filtering (cache-dependent)
- Relation expansion: O(k * d) where k=seeds, d=relation depth (1-3)
- Scoring: O(m) where m=results (~10-100 results typical)

**Deduplication:** O(m log m) for fingerprinting + sorting

**Bottleneck:** Graph traversal for expansions (can hit graph query limits)

