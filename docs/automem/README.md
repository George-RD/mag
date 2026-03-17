# AutoMem — Architecture Overview

**Source**: https://github.com/verygoodplugins/automem
**Benchmarks**: 89.27% on `locomo-mini` (cat 1-4), 87.56% on full LoCoMo with cat-5 judge

## What It Is

AutoMem is a Python Flask service providing long-term AI memory backed by two stores:

- **FalkorDB** (Redis-based graph DB) — canonical memory storage, relationships, entity nodes
- **Qdrant** (vector DB) — embeddings for semantic search, metadata payload for filtering

## Tech Stack

| Component | Technology |
|-----------|------------|
| API server | Flask (Python) |
| Graph store | FalkorDB (Cypher queries) |
| Vector store | Qdrant (COSINE distance) |
| Embedding | OpenAI `text-embedding-3-small` (768-dim default), FastEmbed/ONNX, Ollama, Voyage |
| Classification | `gpt-4o-mini` (regex first, LLM fallback) |
| LLM judge | `gpt-4o` (LoCoMo cat-5 only) |

## Key Architectural Decisions

1. **Dual-write**: Every memory is persisted synchronously to FalkorDB, then embedding queued async to Qdrant. Vector store is a cache; FalkorDB is the source of truth.
2. **Async enrichment**: Entity extraction and graph relationship building run in background after embedding completes (JIT phase ~25-125ms, batch phase ~110-500ms).
3. **Dependency injection**: `runtime_wiring.py::wire_recall_and_blueprints()` injects 50+ helper functions into Flask blueprints — no global state.
4. **Queue-based embedding**: Background thread batches embedding requests (default batch_size=512, flush timeout 30s). Thread-safe skip-if-inflight guard.
5. **Content governance**: Hard limit 2000 chars (reject), soft limit 500 chars (auto-summarize via OpenAI, target 300 chars).
6. **Consolidation scheduler**: Background daemon runs every 60s, dispatching decay/creative/cluster tasks on fixed cycles.

## Module Map

```
automem/
  api/
    memory.py                  — CRUD endpoints (store, get, update, delete, batch, associate)
    runtime_memory_routes.py   — Route handlers with validation
    recall.py                  — Main recall orchestrator (~1750 lines)
    runtime_recall_routes.py   — Flask wrapper for recall
    graph.py                   — Graph visualization endpoints
    enrichment.py              — Enrichment status/reprocess endpoints
    stream.py                  — SSE streaming (subscriber pattern)
  search/
    runtime_recall_helpers.py  — Vector/graph search orchestration
    runtime_keywords.py        — Keyword tokenization (3+ char tokens, stopwords)
    runtime_relations.py       — Multi-hop graph traversal (1-3 depth)
  embedding/
    provider.py                — Abstract EmbeddingProvider base class
    openai.py                  — OpenAI provider
    fastembed.py               — Local ONNX (BAAI/bge-* models)
    runtime_pipeline.py        — Async queue-based embedding worker
  enrichment/
    (entity extraction, graph population)
  consolidation/
    runtime_helpers.py         — Control record, run persistence
    runtime_scheduler.py       — Background consolidation daemon
  classification/
    memory_classifier.py       — 7-type classifier (regex + LLM fallback)
  stores/
    runtime_clients.py         — FalkorDB + Qdrant initialization
  service_runtime.py           — Startup sequence (8 phases)
  runtime_wiring.py            — Dependency injection
  config.py                    — All env-var defaults
```

## Memory Storage Flow

```
POST /memory
  → validate (UUID, content ≤ 2000 chars)
  → auto-summarize if > 500 chars
  → normalize tags + timestamps
  → FalkorDB write (sync)
  → Qdrant embedding queue (async)
  → [background] generate embedding → insert Qdrant payload
  → [background] enrichment pipeline (entities, relationships)
```

## Related Docs

- [search-recall.md](search-recall.md) — Recall pipeline, scoring formula, query decomposition
- [enrichment-graph.md](enrichment-graph.md) — Entity extraction, graph schema, relationship types
- [embedding-consolidation.md](embedding-consolidation.md) — Embedding providers, classification, consolidation cycles
- [locomo-benchmark.md](locomo-benchmark.md) — How AutoMem runs and scores LoCoMo
- [gap-analysis.md](gap-analysis.md) — MAG vs AutoMem feature comparison
