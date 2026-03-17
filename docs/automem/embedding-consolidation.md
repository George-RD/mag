# AutoMem — Embedding & Memory Lifecycle

## Embedding Providers

All providers implement `EmbeddingProvider` (abstract base in `automem/embedding/provider.py`):
- `generate_embedding(text: str) → List[float]`
- `generate_embeddings_batch(texts: List[str]) → List[List[float]]`
- `dimension() → int`

| Provider | File | Notes |
|----------|------|-------|
| OpenAI | `openai.py` | Default model `text-embedding-3-small`, 768-dim. Supports custom `base_url` via `OPENAI_BASE_URL`. Only sends `dimensions` param to native OpenAI API. |
| FastEmbed (ONNX) | `fastembed.py` | Local, no API cost. Auto-selects model by dimension: 384→`BAAI/bge-small-en-v1.5` (67MB), 768→`bge-base-en-v1.5` (210MB), 1024→`bge-large-en-v1.5` (1.2GB). Cache dir: `~/.config/automem/models/` (override: `AUTOMEM_MODELS_DIR`). Probes actual dim on init. |
| Ollama | (local HTTP) | Local inference server |
| Voyage | (API) | Voyage AI hosted embeddings |

Default config: `EMBEDDING_MODEL=text-embedding-3-small`, `VECTOR_SIZE=1024`.

## Async Embedding Pipeline (`runtime_pipeline.py`)

Queue-based background worker:

1. **`init_embedding_pipeline()`** — starts background thread
2. **`enqueue_embedding(memory_id, content)`** — non-blocking enqueue
   - Skips if `memory_id` already pending or inflight (thread-safe lock)
3. **`embedding_worker()`** background loop:
   - Batches up to `batch_size=512` items
   - Flushes on `batch_timeout_seconds=30` even if batch not full
   - Retries on error
4. **`store_embedding_in_qdrant()`** — fetches metadata from FalkorDB, inserts into Qdrant with payload:
   - `content`, `tags`, `tag_prefixes`, `importance`, `timestamp`, `type`, `confidence`, `updated_at`, `last_accessed`, `metadata`, `relevance_score`

Qdrant collection uses COSINE distance. Payload indexes on `tags` and `tag_prefixes` (KEYWORD schema) for fast filtered search.

## Memory Classification (`classification/memory_classifier.py`)

**Strategy**: Regex pattern matching first; optional LLM (`gpt-4o-mini`) fallback.

**7 canonical types with detection patterns:**

| Type | Regex triggers |
|------|----------------|
| `Decision` | "decided to", "chose X over", "picked", "opted for" |
| `Pattern` | "usually", "typically", "often", "frequently" |
| `Preference` | "prefer", "like better", "favorite", "rather than" |
| `Style` | "wrote in style", "communicated", "formatted as" |
| `Habit` | "always", "every time", "daily", "weekly" |
| `Insight` | "realized", "discovered", "learned", "figured out" |
| `Context` | "during", "while working on", "situation was" |

Confidence scoring:
- Base: **0.6** (single pattern match)
- +0.1 per additional matching pattern (capped at **0.95**)

LLM fallback:
- Model: `gpt-4o-mini` (configurable: `CLASSIFICATION_MODEL`)
- Input truncated to 1000 chars
- JSON response format: `{type: str, confidence: float}`
- 20+ legacy alias mappings via `TYPE_ALIASES` for backward compatibility

Protected types that cannot be deleted by consolidation: `Decision`, `Insight`.

## Content Governance

| Limit | Value | Action |
|-------|-------|--------|
| Soft limit | 500 chars | Auto-summarize (target 300 chars via OpenAI) |
| Hard limit | 2000 chars | Reject with 400 error |
| `MEMORY_AUTO_SUMMARIZE` | `true` (default) | Toggle auto-summarization |

When summarized, original content is preserved in memory metadata.

## Consolidation Scheduler (`consolidation/runtime_scheduler.py`)

Background daemon thread, tick interval: `CONSOLIDATION_TICK_SECONDS=60`.

**Four task cycles:**

| Task | Default Interval | What It Does |
|------|-----------------|-------------|
| Decay | 1 day (86400s) | Exponential importance decay for stale memories. Rate: 0.01/day. Floor: 0.3× original importance. |
| Creative | 7 days (604800s) | Synthesizes new meta-memories from pattern clusters. Creates `CONTRASTS_WITH` and `DISCOVERED` edges. |
| Cluster | 30 days (2592000s) | Groups similar memories, creates summary nodes. |
| Forget | **disabled** (0) | Would archive/delete low-relevance memories. |

**Relevance scoring for decay/forget:**
- Accounts for memory age
- Weighted by relationship count (more connected = slower decay)
- Combines importance + confidence
- Range: 0–1

**Forget thresholds (all disabled by default):**
- `CONSOLIDATION_DELETE_THRESHOLD=0.0`
- `CONSOLIDATION_ARCHIVE_THRESHOLD=0.0`
- `CONSOLIDATION_GRACE_PERIOD_DAYS=90`
- `CONSOLIDATION_IMPORTANCE_PROTECTION_THRESHOLD=0.7` (above this → never deleted)

**Run persistence (`runtime_helpers.py`):**
- `load_control_record()` — fetches/creates singleton `ConsolidationControl` node in FalkorDB
- `persist_consolidation_run()` — creates `ConsolidationRun` node, updates control timestamps, prunes history (keep last 20 runs)
- `build_consolidator_from_config()` — factory that reads all env vars and constructs the consolidator object

**Metrics emitted per tick:**
- `task_type`, `affected_count`, `elapsed_ms`, `success`, `next_scheduled`
