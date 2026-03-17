# AutoMem vs MAG — Gap Analysis

MAG = this repo (`romega-memory`), a Rust MCP server with SQLite + ONNX embeddings.

## Feature Comparison

| Feature | MAG | AutoMem |
|---------|-----|---------|
| **Storage** | SQLite (single file) | FalkorDB + Qdrant (two services) |
| **Embeddings** | ONNX local (bge-small, 384-dim) | OpenAI / FastEmbed / Ollama / Voyage (768-1024-dim) |
| **Vector search** | sqlite-vec + FTS5 BM25 | Qdrant COSINE |
| **Graph relationships** | SQLite graph table, BFS traversal | FalkorDB Cypher, 13 relationship types |
| **Entity extraction** | None | spaCy NER + regex (5 categories) |
| **Graph enrichment** | Explicit associations only | Auto-extracted entities + temporal + similarity |
| **Pattern detection** | None | Pattern nodes from 3+ similar memories |
| **Temporal links** | None | `PRECEDED_BY` within 7-day window |
| **Memory classification** | Intent classification for search | 7-type classifier (regex + gpt-4o-mini fallback) |
| **Consolidation** | TTL sweep + feedback scoring | Decay (1d), creative (7d), cluster (30d), forget (off) |
| **Content limits** | None explicit | Hard 2000 chars, soft 500 chars (auto-summarize) |
| **Recall pipeline** | 6-phase RRF (vector + FTS5 + rerank + graph + abstention) | Vector + graph keyword + tag fallback + 10-component scoring |
| **Query decomposition** | Intent classification only | Entity + topic sub-query generation for multi-hop |
| **Relation expansion** | Graph BFS (max_hops param) | Graph traversal with edge strength weighting, boost=0.25 |
| **Context bonuses** | Type weight × priority × word_overlap | Tag hit +0.45, type match +0.25, anchor ID +0.90 |
| **Multi-hop recall** | Graph BFS from initial results | `multi_hop_recall_with_graph()` initial_limit=20, max_connected=60 |
| **LoCoMo scoring** | Word-overlap (same 0.5 threshold) | Word-overlap (0.5) + embedding sim (0.50) + GPT-4o judge (cat-5) |
| **Streaming** | None | SSE endpoint with subscriber pattern |
| **Batch ingest** | None | `POST /memory/batch` up to 500 per call |
| **Infrastructure** | Zero dependencies (embedded) | Requires FalkorDB + Qdrant + Python services |

## Key Gaps That Affect LoCoMo Scores

### 1. No entity extraction or entity-based graph expansion

AutoMem's highest-impact feature for LoCoMo: entity nodes link memories about the same person/project across separate conversations. When asked "What did Caroline decide?", AutoMem can find all `INVOLVES:Caroline` edges and surface them even if none are the top vector hit.

**MAG gap**: associations are explicit (user-created) only. No auto-linking of same-entity memories.

### 2. No query decomposition for multi-hop questions

AutoMem generates sub-queries from the original question (entity extraction + compound queries). For cat-3 questions like "Would X's friend pursue Y?", this creates sub-queries for each entity separately before merging results.

**MAG gap**: only intent classification, no sub-query generation. Multi-hop relies entirely on graph BFS from initial results, which requires the initial hit to be on the right memory.

### 3. No temporal relationship links

`PRECEDED_BY` edges let AutoMem answer "What happened around the time of X?" by graph traversal without requiring semantic overlap. LoCoMo cat-2 (temporal) questions benefit directly.

**MAG gap**: time filters exist but no stored temporal adjacency edges.

### 4. Pattern nodes for recurring behaviors

AutoMem builds `Pattern` nodes when 3+ similar memories exist, and uses them during creative consolidation to surface generalizations. Helps with preference/habit questions in LoCoMo.

**MAG gap**: no pattern detection or generalization layer.

## What AutoMem Does That MAG Doesn't Need

- **Two separate services** (FalkorDB + Qdrant): MAG's SQLite + sqlite-vec achieves comparable functionality with zero ops overhead. For a local MCP server this is the right tradeoff.
- **Cloud/Railway deployment support**: env-var detection for Railway domains. MAG targets local CLI use.
- **SSE streaming**: MCP protocol handles this differently.
- **Content auto-summarization**: useful for a multi-tenant service; for personal memory the 2000-char limit may never trigger.
- **gpt-4o-mini classification**: regex-based type detection is sufficient for MAG's 7 types.
- **Solana token ($AUTOMEM)**: marketing feature; not relevant.

## What MAG Can Learn/Adopt

| Idea | Effort | Impact |
|------|--------|--------|
| Sub-query generation for multi-hop (decompose entity + topic) | Medium | High for cat-3 |
| Entity extraction → auto-tag memories with `entity:person:*` | High | High for cat-1/3 |
| Temporal adjacency edges (within N days) | Low | Medium for cat-2 |
| Context anchor bonus (force priority IDs to top) | Low | Medium |
| Edge strength weighting in relation expansion (not just BFS) | Low | Low |
| Pattern node detection from FTS5 clusters | Medium | Low |
