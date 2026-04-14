# MAG Substrate Trait Surface Design Spec

**Status:** Draft  
**Module:** `substrate/` (new, alongside `memory_core/`)  
**Scope:** 7 swap-point traits + supporting types, relationship mapping, composition patterns, reference impls, and deprecation path.

---

## 1. Motivation

`SqliteStorage` currently implements ~28 fine-grained traits but has no internal swap points. Every algorithmic concern — candidate collection, fusion, scoring, graph enrichment, lifecycle — is coupled directly to SQLite. This spec defines a `substrate/` module with 7 clean interfaces that can be implemented by alternative backends, mixed-in, or replaced at runtime without touching call-site code.

---

## 2. Supporting Types

These types live in `substrate::types` and bridge to existing `memory_core` types.

```rust
use crate::memory_core::domain::{SearchOptions, SemanticResult, MemoryInput, EventType};
use crate::memory_core::scoring::ScoringParams;
use std::collections::HashMap;

/// A candidate memory with accumulated score. Bridges to the private
/// `RankedSemanticCandidate` used inside `SqliteStorage`.
///
/// Public mirror of the internal struct so substrate impls can work with it
/// without depending on the SQLite module internals.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    /// The underlying search result (id, content, tags, importance, metadata,
    /// event_type, session_id, project, entity_id, agent_type, score).
    pub result: SemanticResult,
    /// ISO 8601 wall-clock creation timestamp.
    pub created_at: String,
    /// ISO 8601 event timestamp (may differ from created_at for backdated events).
    pub event_at: String,
    /// Accumulated composite score (mutable through the pipeline).
    pub score: f64,
    /// Resolved priority (0-4) used by scorer chain.
    pub priority_value: u8,
    /// Raw cosine similarity from vector search, if this candidate came from
    /// the vector path. None for FTS-only candidates.
    pub vec_sim: Option<f64>,
    /// Word overlap fraction computed during score refinement.
    pub text_overlap: f64,
    /// Denormalised entity_id for entity expansion scorer.
    pub entity_id: Option<String>,
    /// Denormalised agent_type for in-memory filtering.
    pub agent_type: Option<String>,
    /// Populated only when `SearchOptions::explain` is true.
    pub explain: Option<serde_json::Value>,
}

/// Type alias preserving the existing name used internally in `SqliteStorage`.
/// Allows migration: old code keeps compiling; new code can use `ScoredCandidate`.
pub type RankedSemanticCandidate = ScoredCandidate;

/// An ordered, keyed set of candidates produced by a `RetrievalStrategy`.
/// The key is the strategy name (e.g. `"vector"`, `"fts"`).
pub type CandidateSet = Vec<(String, f64, ScoredCandidate)>;

/// Read-path context passed through the pipeline.
///
/// Replaces the scattered `query`, `limit`, `opts`, `scoring_params`
/// parameter tuples used throughout `advanced.rs`.
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// Raw query string from the caller.
    pub query: String,
    /// Maximum number of results to return after the full pipeline.
    pub limit: usize,
    /// Filter and feature options (event_type, project, session, explain, etc.).
    pub opts: SearchOptions,
    /// Scoring knobs. Consumers should clone from a shared `Arc<ScoringParams>`.
    pub scoring_params: ScoringParams,
    /// Pre-computed query embedding. `None` until the ingestion/embedding stage
    /// populates it; strategies that do not need embeddings ignore it.
    pub query_embedding: Option<Vec<f32>>,
    /// Derived token set (stemmed, stop-word-filtered) for word-overlap scoring.
    /// Populated lazily by the pipeline orchestrator before calling scorers.
    pub query_tokens: Option<std::collections::HashSet<String>>,
    /// Whether superseded memories should be included in candidate sets.
    pub include_superseded: bool,
}

/// Write-path context for `IngestionPipeline`.
#[derive(Debug, Clone)]
pub struct WriteContext {
    pub input: MemoryInput,
    pub assigned_id: String,
    pub embedding: Option<Vec<f32>>,
}

/// Result returned by `ConsolidationStrategy::run`.
#[derive(Debug, Clone)]
pub struct ConsolidationReport {
    pub strategy: String,
    pub memories_examined: usize,
    pub memories_modified: usize,
    pub dry_run: bool,
    pub detail: serde_json::Value,
}
```

---

## 3. The 7 Swap-Point Traits

### 3.1 Storage — Physical CRUD Backend

Replaces/extends the existing `Storage` trait in `traits.rs`. The supertrait family
bundles the 28 existing traits into a single `MemoryStore` supertrait so backends
can be swapped atomically.

```rust
use async_trait::async_trait;
use anyhow::Result;
use crate::memory_core::domain::{
    BackupInfo, CheckpointInput, GraphNode, ListResult, MemoryInput, MemoryUpdate,
    Relationship, SearchOptions, SearchResult, SemanticResult, WelcomeOptions,
};

/// Physical CRUD + ancillary ops. Implemented by `SqliteStorage` today.
/// Every method mirrors an existing trait in `memory_core/traits.rs` so
/// blanket impls can delegate automatically (see §6).
#[async_trait]
pub trait MemoryStore: Send + Sync {
    // ── Core CRUD ─────────────────────────────────────────────────────────
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()>;
    async fn retrieve(&self, id: &str) -> Result<String>;
    async fn delete(&self, id: &str) -> Result<bool>;
    async fn update(&self, id: &str, update: &MemoryUpdate) -> Result<()>;

    // ── Query surface ─────────────────────────────────────────────────────
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>>;
    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>>;
    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>>;
    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
    async fn list(
        &self,
        offset: usize,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<ListResult>;

    // ── Graph ─────────────────────────────────────────────────────────────
    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>>;
    async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>>;
    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>>;

    // ── Versioning ────────────────────────────────────────────────────────
    async fn get_version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>>;
    async fn supersede_memory(&self, old_id: &str, new_id: &str) -> Result<()>;

    // ── Lifecycle ancillaries ─────────────────────────────────────────────
    async fn sweep_expired(&self) -> Result<usize>;
    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value>;

    // ── Profile / checkpoint / reminder / lesson ──────────────────────────
    async fn get_profile(&self) -> Result<serde_json::Value>;
    async fn set_profile(&self, updates: &serde_json::Value) -> Result<()>;
    async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String>;
    async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>>;
    async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;
    async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>>;
    async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value>;
    async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>>;

    // ── Maintenance / stats ───────────────────────────────────────────────
    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value>;
    async fn consolidate(
        &self,
        prune_days: i64,
        max_summaries: i64,
    ) -> Result<serde_json::Value>;
    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value>;
    async fn clear_session(&self, session_id: &str) -> Result<usize>;
    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value>;
    async fn type_stats(&self) -> Result<serde_json::Value>;
    async fn session_stats(&self) -> Result<serde_json::Value>;
    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value>;
    async fn access_rate_stats(&self) -> Result<serde_json::Value>;

    // ── Backup ────────────────────────────────────────────────────────────
    async fn create_backup(&self) -> Result<BackupInfo>;
    async fn rotate_backups(&self, max_count: usize) -> Result<usize>;
    async fn list_backups(&self) -> Result<Vec<BackupInfo>>;
    async fn restore_backup(&self, backup_path: &std::path::Path) -> Result<()>;
    async fn maybe_startup_backup(&self) -> Result<Option<BackupInfo>>;

    // ── Welcome ───────────────────────────────────────────────────────────
    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;
}
```

---

### 3.2 RetrievalStrategy — Unscored Candidate Collection

Produces a raw candidate list from one retrieval signal (vector OR FTS).
Analogous to the internal `collect_vector_candidates` / `collect_fts_candidates`
functions in `advanced.rs`.

```rust
/// Retrieves an unscored, unranked candidate set for a single retrieval signal.
///
/// Implementors MUST NOT apply multi-signal fusion — that is `FusionStrategy`'s job.
/// The returned `CandidateSet` is `(memory_id, raw_score, candidate)` where
/// `raw_score` is signal-native (cosine similarity for vector; raw BM25 for FTS,
/// where more-negative = better).
#[async_trait]
pub trait RetrievalStrategy: Send + Sync {
    /// Human-readable name used as the key in `FusionStrategy::fuse`.
    fn name(&self) -> &str;

    /// Collect candidates for the given context.
    ///
    /// Implementors that perform blocking I/O (e.g. ONNX inference, SQLite reads)
    /// MUST wrap the blocking work in `tokio::task::spawn_blocking`.
    async fn collect(
        &self,
        ctx: &QueryContext,
    ) -> Result<CandidateSet>;
}

/// Reference implementation: vector (embedding) similarity search.
///
/// Wraps `collect_vector_candidates` from `advanced.rs`.
/// Requires the `real-embeddings` or `sqlite-vec` feature.
pub struct VectorSearch {
    pub store: std::sync::Arc<dyn MemoryStore>,
    // Internal: holds the embedder + connection pool refs via SqliteStorage.
}

/// Reference implementation: BM25 full-text search.
///
/// Wraps `collect_fts_candidates` from `advanced.rs`.
pub struct FullTextSearch {
    pub store: std::sync::Arc<dyn MemoryStore>,
}
```

---

### 3.3 FusionStrategy — Multi-Signal Merging

Takes the per-strategy candidate maps and returns a single merged, score-ordered list.

```rust
/// Merges multiple `CandidateSet`s into a single ranked list.
///
/// The `strategy` key in `candidates` matches `RetrievalStrategy::name()`.
/// Fusion is pure in-memory arithmetic — no I/O allowed.
pub trait FusionStrategy: Send + Sync {
    /// Merge candidates from all strategies into one scored list.
    ///
    /// Returns candidates keyed by memory id, in descending score order.
    fn fuse(
        &self,
        candidates: HashMap<&str, CandidateSet>,
        scoring_params: &ScoringParams,
    ) -> Vec<ScoredCandidate>;
}

/// Reference implementation: Reciprocal Rank Fusion with adaptive dual-match boost.
///
/// Direct extraction of the RRF logic in `fuse_refine_and_output` (advanced.rs:868+).
///
/// Algorithm:
///   rrf_score(rank) = weight / (k + rank + 1)
///   dual-match boost = base_boost + (1/(1+fts_rank)) * 0.5
///   where base_boost = scoring_params.dual_match_boost (default 1.5)
///         k = scoring_params.rrf_k (default 60.0)
///
/// Weights per strategy:
///   "vector" → scoring_params.rrf_weight_vec (default 1.0)
///   "fts"    → scoring_params.rrf_weight_fts (default 1.0)
pub struct RrfFusion;
```

---

### 3.4 Scorer — Composable Post-Fusion Scoring Chain

Each `Scorer` takes the fused candidate map and mutates scores in-place.
Scorers are chained; each sees the output of the previous one.

```rust
/// Post-fusion score refinement. One unit of composable scoring logic.
///
/// `score_batch` MUST be pure in-memory for hot-path scorers. Scorers that
/// need I/O (cross-encoder, graph queries) MUST use `spawn_blocking` internally.
#[async_trait]
pub trait Scorer: Send + Sync {
    /// Human-readable name for logging/explain output.
    fn name(&self) -> &str;

    /// Mutate scores on a batch of candidates in-place.
    async fn score_batch(
        &self,
        candidates: &mut HashMap<String, ScoredCandidate>,
        ctx: &QueryContext,
    ) -> Result<()>;
}

/// Reference: MultiFactorScorer
///
/// Applies word overlap, query coverage boost, Jaccard similarity, feedback
/// dampening, time decay, importance floor/scale, and context-tag matching.
/// Direct extraction of `refine_scores` (advanced.rs:370-448).
///
/// Score multiplications applied in order:
///   1. coverage_boost   = 1.0 + (query_coverage_boost(overlap) - 1.0) * fb_dampening
///   2. word_overlap     *= 1.0 + overlap * scoring_params.word_overlap_weight * fb_dampening
///   3. jaccard          *= 1.0 + jaccard * scoring_params.jaccard_weight
///   4. feedback_factor  (see scoring.rs:feedback_factor)
///   5. time_decay_et    (no decay for Semantic kind memories)
///   6. importance_factor = importance_floor + importance * importance_scale
///   7. context_tag ratio *= 1.0 + ratio * context_tag_weight
pub struct MultiFactorScorer {
    // No fields required; all parameters come from QueryContext::scoring_params.
}

/// Reference: CrossEncoderScorer
///
/// Wraps `CrossEncoderReranker::score_batch` (reranker.rs).
/// Model: ms-marco-MiniLM-L-6-v2 (ONNX, CPU-only, auto-downloaded).
/// Scores top `scoring_params.rerank_top_n` (default 30) candidates.
/// Blends: final = alpha * rrf_score + (1-alpha) * cross_encoder_score
///         where alpha = scoring_params.rerank_blend_alpha (default 0.5).
/// MUST wrap `CrossEncoderReranker::score_batch` in `spawn_blocking`.
/// Feature-gated: only compiled with `#[cfg(feature = "real-embeddings")]`.
#[cfg(feature = "real-embeddings")]
pub struct CrossEncoderScorer {
    pub reranker: std::sync::Arc<crate::memory_core::reranker::CrossEncoderReranker>,
}

/// Reference: GraphNeighborScorer
///
/// Injects 1-hop graph neighbors from the top-k seed candidates.
/// Direct extraction of `enrich_graph_neighbors` (advanced.rs:450-660).
/// Seeds: top `limit.clamp(graph_seed_min, graph_seed_max)` by score.
/// Neighbor score = graph_neighbor_factor * seed_score * edge_weight,
/// with edge-type boosts:
///   REL_PRECEDED_BY → * scoring_params.preceded_by_boost (default 1.5)
///   REL_RELATES_TO / REL_SIMILAR_TO / REL_SHARES_THEME / REL_PARALLEL_CONTEXT
///                   → * scoring_params.entity_relation_boost (default 1.3)
/// Requires a SQLite connection for neighbor lookup — wraps in `spawn_blocking`.
pub struct GraphNeighborScorer {
    pub store: std::sync::Arc<dyn MemoryStore>,
}

/// Reference: EntityExpansionScorer
///
/// Extracts entity tags from top seeds, queries memories sharing those tags,
/// injects them with ENTITY_EXPANSION_BOOST (1.15×).
/// Direct extraction of `expand_entity_candidates` (advanced.rs:678-866).
/// Caps expansion at 25 additional memories, 5 entity tags, 5 per tag.
pub struct EntityExpansionScorer {
    pub store: std::sync::Arc<dyn MemoryStore>,
}
```

---

### 3.5 LifecyclePolicy — TTL / Promotion / Decay

Controls which memories survive, expire, or get promoted over time.

```rust
/// Determines what happens to a memory at lifecycle checkpoints.
#[async_trait]
pub trait LifecyclePolicy: Send + Sync {
    fn name(&self) -> &str;

    /// Called on memory read: returns `true` if the memory should be served
    /// (i.e. has not expired). Non-destructive gate.
    fn is_alive(&self, candidate: &ScoredCandidate) -> bool;

    /// Sweep all expired memories from the store. Returns count removed.
    /// Wraps `ExpirationSweeper::sweep_expired` from `traits.rs`.
    async fn sweep(&self, store: &dyn MemoryStore) -> Result<usize>;

    /// Apply decay or promotion mutations to a candidate's score.
    /// Called inside the scorer chain when lifecycle context is available.
    fn apply_decay(&self, candidate: &mut ScoredCandidate, now_secs: u64);
}

/// Reference: TtlExpirationPolicy
///
/// Uses the `ttl_seconds` field from `MemoryInput` and `EventType::default_ttl()`.
/// Delegates sweep to `MemoryStore::sweep_expired`.
/// `is_alive` checks metadata["expires_at"] < now.
/// `apply_decay` is a no-op (TTL is binary alive/dead, not graduated decay).
pub struct TtlExpirationPolicy;
```

---

### 3.6 ConsolidationStrategy — Background Memory Restructuring

Runs off the hot path. Each strategy is one unit of background compaction logic.

```rust
/// One self-contained consolidation pass over the memory store.
#[async_trait]
pub trait ConsolidationStrategy: Send + Sync {
    fn name(&self) -> &str;

    /// Run one consolidation pass.
    ///
    /// `dry_run = true` MUST produce the same `ConsolidationReport` but make
    /// no mutations to the store.
    async fn run(
        &self,
        store: &dyn MemoryStore,
        dry_run: bool,
    ) -> Result<ConsolidationReport>;
}

/// Reference: DedupConsolidation
///
/// Merges near-duplicate memories per event type using Jaccard similarity.
/// Wraps `MemoryStore::compact`.
/// Uses `EventType::types_with_dedup_threshold()` to find eligible types.
/// Thresholds from `EventType::dedup_threshold()` (0.70-0.85 range).
pub struct DedupConsolidation {
    pub min_cluster_size: usize,
}

/// Reference: CompactConsolidation
///
/// Prunes zero-access memories older than a threshold and caps session summaries.
/// Wraps `MemoryStore::consolidate`.
pub struct CompactConsolidation {
    pub prune_days: i64,
    pub max_summaries: i64,
}

/// Reference: AutoRelateConsolidation
///
/// Auto-compact pass across all event types with dedup thresholds,
/// triggered when total memory count exceeds a threshold.
/// Wraps `MemoryStore::auto_compact`.
pub struct AutoRelateConsolidation {
    pub count_threshold: usize,
}
```

---

### 3.7 IngestionPipeline — Write-Path Processing

The write-path equivalent of `SearchPipeline`. Handles embedding, dedup detection,
entity extraction, and auto-supersession before the store write.

```rust
/// Write-path processor: takes a `WriteContext`, applies transformations,
/// and writes to the store.
#[async_trait]
pub trait IngestionPipeline: Send + Sync {
    /// Process and store a memory. Returns the assigned memory ID.
    ///
    /// Implementors are responsible for:
    ///   1. Assigning an ID if `WriteContext::assigned_id` is empty.
    ///   2. Computing the embedding (spawn_blocking for ONNX).
    ///   3. Dedup / auto-supersession checks.
    ///   4. Entity extraction from tags.
    ///   5. Calling `MemoryStore::store`.
    async fn ingest(
        &self,
        ctx: WriteContext,
        store: &dyn MemoryStore,
    ) -> Result<String>;
}

/// Reference: EmbedAndExtractPipeline
///
/// Mirrors the write path in `SqliteStorage`:
///   1. Normalise input (apply_event_type_defaults).
///   2. Compute embedding via `Embedder::embed` (spawn_blocking).
///   3. Check cosine similarity for auto-supersession
///      (threshold: SUPERSESSION_COSINE_THRESHOLD = 0.70,
///       secondary Jaccard: SUPERSESSION_JACCARD_THRESHOLD = 0.30).
///   4. Check content hash for exact dedup.
///   5. Extract entities from tags for relationship edges.
///   6. Call MemoryStore::store.
///   7. Create PRECEDED_BY, RELATES_TO relationship edges.
pub struct EmbedAndExtractPipeline {
    pub embedder: std::sync::Arc<dyn crate::memory_core::embedder::Embedder>,
}
```

---

## 4. Orchestrator Types

These wire the 7 traits together at runtime.

### 4.1 SearchPipeline

```rust
/// The read-path orchestrator. Wires retrieval → fusion → scoring → lifecycle.
///
/// Callers construct this once (at daemon startup or per-request for testing)
/// and call `search`.
pub struct SearchPipeline {
    /// Retrieval strategies, run concurrently (tokio::join! or FuturesUnordered).
    pub retrieval: Vec<Box<dyn RetrievalStrategy>>,
    /// Single fusion pass applied after all retrieval strategies complete.
    pub fusion: Box<dyn FusionStrategy>,
    /// Scorer chain — applied in order, each sees the previous scorer's output.
    pub scorers: Vec<Box<dyn Scorer>>,
    /// Lifecycle gate applied after scoring; filters dead memories.
    pub lifecycle: Option<Box<dyn LifecyclePolicy>>,
    /// Abstention threshold on max text overlap before returning empty.
    /// Matches ABSTENTION_MIN_TEXT (0.15) from scoring.rs.
    pub abstention_min_text: f64,
}

impl SearchPipeline {
    /// Execute the full pipeline for a query.
    ///
    /// Steps:
    ///   1. Run all `retrieval` strategies concurrently.
    ///   2. Pass strategy→CandidateSet map to `fusion.fuse`.
    ///   3. Apply each scorer in `scorers` in order.
    ///   4. Apply `lifecycle.is_alive` filter if set.
    ///   5. Apply abstention gate (max text_overlap < abstention_min_text → return empty).
    ///   6. Sort descending by score, truncate to `ctx.limit`.
    ///   7. Map to `SemanticResult`.
    pub async fn search(&self, ctx: QueryContext) -> Result<Vec<SemanticResult>> {
        // ... (implementation in Phase 3)
        todo!()
    }
}
```

### 4.2 WritePipeline

```rust
/// The write-path orchestrator. Thin wrapper around `IngestionPipeline`.
pub struct WritePipeline {
    pub pipeline: Box<dyn IngestionPipeline>,
    pub store: std::sync::Arc<dyn MemoryStore>,
}

impl WritePipeline {
    pub async fn ingest(&self, input: MemoryInput) -> Result<String> {
        let ctx = WriteContext {
            assigned_id: input.id.clone().unwrap_or_default(),
            embedding: None,
            input,
        };
        self.pipeline.ingest(ctx, self.store.as_ref()).await
    }
}
```

### 4.3 ConsolidationRunner

```rust
/// Runs consolidation strategies in registration order.
pub struct ConsolidationRunner {
    pub strategies: Vec<Box<dyn ConsolidationStrategy>>,
    pub store: std::sync::Arc<dyn MemoryStore>,
}

impl ConsolidationRunner {
    pub async fn run_all(&self, dry_run: bool) -> Result<Vec<ConsolidationReport>> {
        let mut reports = Vec::with_capacity(self.strategies.len());
        for strategy in &self.strategies {
            reports.push(strategy.run(self.store.as_ref(), dry_run).await?);
        }
        Ok(reports)
    }
}
```

---

## 5. Existing Trait → New Hierarchy Mapping

Each of the 28 traits in `memory_core/traits.rs` lands in exactly one location in the new hierarchy.

| Existing trait (`traits.rs`) | New home | Notes |
|---|---|---|
| `Ingestor` | `IngestionPipeline` | Subsumed; `ingest(&str)` becomes `ingest(WriteContext)` |
| `Processor` | `IngestionPipeline` | Subsumed; embedding+NLP are pipeline steps |
| `Storage` | `MemoryStore::store` | Signature unchanged; renamed to avoid confusion with the module |
| `Retriever` | `MemoryStore::retrieve` | Point lookup, not retrieval strategy |
| `Searcher` | `MemoryStore::search` + `SearchPipeline` | BM25/keyword search delegated to `FullTextSearch` |
| `Recents` | `MemoryStore::recent` | Unchanged; no pipeline involvement |
| `SemanticSearcher` | `MemoryStore::semantic_search` | Simple semantic path; `SearchPipeline` is the advanced path |
| `GraphTraverser` | `MemoryStore::traverse` | Direct graph query; no scorer involvement |
| `SimilarFinder` | `MemoryStore::find_similar` | Direct lookup by embedding similarity |
| `PhraseSearcher` | `MemoryStore::phrase_search` | Direct FTS phrase query |
| `AdvancedSearcher` | `SearchPipeline::search` | Primary target; pipeline replaces monolithic impl |
| `Deleter` | `MemoryStore::delete` | Unchanged |
| `Updater` | `MemoryStore::update` | Unchanged |
| `Tagger` | `MemoryStore::get_by_tags` | Unchanged; also feeds `EntityExpansionScorer` |
| `Lister` | `MemoryStore::list` | Unchanged |
| `RelationshipQuerier` | `MemoryStore::get_relationships` | Unchanged; also feeds `GraphNeighborScorer` |
| `VersionChainQuerier` | `MemoryStore::{get_version_chain, supersede_memory}` | Unchanged |
| `FeedbackRecorder` | `MemoryStore::record_feedback` | Score impact via `MultiFactorScorer::feedback_factor` |
| `ExpirationSweeper` | `LifecyclePolicy::sweep` | Delegated to `TtlExpirationPolicy` |
| `ProfileManager` | `MemoryStore::{get_profile, set_profile}` | Unchanged |
| `CheckpointManager` | `MemoryStore::{save_checkpoint, resume_task}` | Unchanged |
| `ReminderManager` | `MemoryStore::{create_reminder, list_reminders, dismiss_reminder}` | Unchanged |
| `LessonQuerier` | `MemoryStore::query_lessons` | Unchanged |
| `MaintenanceManager` | `MemoryStore::{check_health, consolidate, compact, clear_session, auto_compact}` + `ConsolidationRunner` | Hot-path ops on `MemoryStore`; scheduled ops via `ConsolidationRunner` |
| `WelcomeProvider` | `MemoryStore::welcome` | Unchanged |
| `BackupManager` | `MemoryStore::{create_backup, rotate_backups, list_backups, restore_backup, maybe_startup_backup}` | Unchanged |
| `StatsProvider` | `MemoryStore::{type_stats, session_stats, weekly_digest, access_rate_stats}` | Unchanged |

---

## 6. Backward Compatibility: Blanket Impls

Blanket impls allow all existing call sites to keep using the granular traits unchanged.
No call sites need modification until the explicit deprecation phase.

```rust
// In memory_core/traits.rs or a new compat.rs:

use crate::substrate::MemoryStore;

/// Any type that implements MemoryStore also implements the legacy Storage trait.
#[async_trait]
impl<T: MemoryStore + ?Sized> crate::memory_core::Storage for T {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        MemoryStore::store(self, id, data, input).await
    }
}

#[async_trait]
impl<T: MemoryStore + ?Sized> crate::memory_core::AdvancedSearcher for T {
    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        MemoryStore::advanced_search(self, query, limit, opts).await
    }
}

// ... one blanket impl per legacy trait, all delegating to MemoryStore methods.
```

`SqliteStorage` continues to implement all legacy traits directly (no code change needed)
because it will grow a `MemoryStore` impl that the blankets delegate through.

---

## 7. Reference Implementation Wiring

The default `SearchPipeline` for production use:

```rust
pub fn default_search_pipeline(store: Arc<SqliteStorage>) -> SearchPipeline {
    let mut scorers: Vec<Box<dyn Scorer>> = vec![
        Box::new(MultiFactorScorer),
    ];

    #[cfg(feature = "real-embeddings")]
    if let Some(reranker) = store.reranker() {
        scorers.push(Box::new(CrossEncoderScorer { reranker }));
    }

    scorers.push(Box::new(GraphNeighborScorer { store: store.clone() }));
    scorers.push(Box::new(EntityExpansionScorer { store: store.clone() }));

    SearchPipeline {
        retrieval: vec![
            Box::new(VectorSearch { store: store.clone() }),
            Box::new(FullTextSearch { store: store.clone() }),
        ],
        fusion: Box::new(RrfFusion),
        scorers,
        lifecycle: Some(Box::new(TtlExpirationPolicy)),
        abstention_min_text: crate::memory_core::scoring::ABSTENTION_MIN_TEXT,
    }
}

pub fn default_consolidation_runner(store: Arc<SqliteStorage>) -> ConsolidationRunner {
    ConsolidationRunner {
        store: store.clone(),
        strategies: vec![
            Box::new(DedupConsolidation { min_cluster_size: 2 }),
            Box::new(CompactConsolidation { prune_days: 30, max_summaries: 50 }),
            Box::new(AutoRelateConsolidation { count_threshold: 500 }),
        ],
    }
}
```

---

## 8. Implementation Phases

### Phase 1 — Define (non-breaking, additive only)
- Create `src/substrate/mod.rs` with all 7 traits and supporting types.
- Export `ScoredCandidate` and `RankedSemanticCandidate` type alias from `substrate`.
- No changes to `memory_core/`.
- Gate on a `substrate` feature flag (disabled by default) to avoid breaking the build.

### Phase 2 — Implement (parallel to existing code)
- Implement `MemoryStore` for `SqliteStorage` by delegating to existing trait impls.
- Implement `VectorSearch` and `FullTextSearch` by extracting `collect_vector_candidates`
  and `collect_fts_candidates` from `advanced.rs` into the new structs.
- Implement `RrfFusion` by extracting the RRF block from `fuse_refine_and_output`.
- Implement `MultiFactorScorer` by extracting `refine_scores`.
- Implement `CrossEncoderScorer` (feature-gated `real-embeddings`) by wrapping reranker.
- Implement `GraphNeighborScorer` by extracting `enrich_graph_neighbors`.
- Implement `EntityExpansionScorer` by extracting `expand_entity_candidates`.
- Implement `TtlExpirationPolicy`, `DedupConsolidation`, `CompactConsolidation`,
  `AutoRelateConsolidation`.
- Implement `EmbedAndExtractPipeline`.
- All implementations are tested in isolation. Existing integration tests still pass.

### Phase 3 — Wire (SearchPipeline goes live)
- `SqliteStorage::advanced_search` delegates to `SearchPipeline::search`.
- `SearchPipeline` calls the extracted strategy/scorer impls.
- Enable `substrate` feature by default.
- Add blanket impls for legacy traits → `MemoryStore`.
- Benchmark to confirm no regression against `ABSTENTION_MIN_TEXT` / LoCoMo-10 baseline.

### Phase 4 — Deprecate (next minor release)
- Add `#[deprecated]` to each of the 28 granular legacy traits.
- Update internal usages to use `MemoryStore` or pipeline types directly.
- Document migration path in CHANGELOG.

### Phase 5 — Remove (next breaking release, semver major or explicit breaking minor)
- Remove the 28 legacy traits from `memory_core/traits.rs`.
- Remove blanket impl shims.
- `MemoryStore` + `substrate` types are the sole interface.

---

## 9. Critical Implementation Details

### spawn_blocking Rules

All CPU-bound or blocking I/O work MUST use `tokio::task::spawn_blocking`:

| Operation | Requirement |
|---|---|
| ONNX embedding inference (`Embedder::embed`) | `spawn_blocking` always |
| Cross-encoder `score_batch` | `spawn_blocking` always |
| SQLite reads via `rusqlite` | `spawn_blocking` inside async contexts |
| SQLite writes via `rusqlite` | `spawn_blocking` always |
| `CrossEncoderReranker::warmup` | Already uses `spawn_blocking` (see reranker.rs:76) |

`RetrievalStrategy::collect` is `async` specifically to allow implementors to
call `spawn_blocking` internally without requiring callers to know.

### Feature Gating

```
real-embeddings → CrossEncoderScorer, CrossEncoderReranker, VectorSearch (knn path)
sqlite-vec      → vec_knn_search, vec_upsert, vec_delete (inside VectorSearch)
```

Without `real-embeddings`, `VectorSearch` falls back to full-scan cosine similarity
(existing non-vec path in `advanced.rs`). Without both features, only `FullTextSearch`
is available; `SearchPipeline` still compiles and works.

### Query Cache

`SqliteStorage` maintains a `QueryCache` (LRU, 128 entries, 60s TTL) keyed on a hash
of `(query, limit, opts)`. The `SearchPipeline` does NOT own the cache — it remains
on `SqliteStorage`. This means:

- Cache hits bypass the pipeline entirely (return early from `advanced_search`).
- Cache invalidation on writes (`invalidate_cache_selective`) continues to work
  because writes go through `MemoryStore::store` which has access to the cache.
- Pipeline-level caching is a future concern; do not add it in Phase 2-3.

### Abstention Gate

After scoring, if `max(text_overlap)` across all surviving candidates is less than
`ABSTENTION_MIN_TEXT` (0.15), the pipeline returns an empty result set rather than
surfacing noise. This gate lives in `SearchPipeline::search`, not in any individual
scorer.

### Hot Cache

`SqliteStorage`'s hot-tier cache (in-memory promotion of frequently-accessed memories)
is backend-specific. It is NOT modelled in the `MemoryStore` trait — it's an
`SqliteStorage` implementation detail that survives the migration unchanged.

### ScoringParams Sharing

`ScoringParams` should be wrapped in `Arc<ScoringParams>` at the daemon level and
passed into `QueryContext` by cloning the `Arc`. The `Default` impl provides production
defaults; tests can override individual fields. Do not pass `ScoringParams` by value
through the pipeline — clone the Arc, not the struct.

---

## 10. Module Layout

```
src/
  substrate/
    mod.rs          — pub use of all traits and types; feature gate
    types.rs        — ScoredCandidate, CandidateSet, QueryContext, WriteContext, ConsolidationReport
    store.rs        — MemoryStore trait
    retrieval.rs    — RetrievalStrategy + VectorSearch + FullTextSearch
    fusion.rs       — FusionStrategy + RrfFusion
    scorer.rs       — Scorer + MultiFactorScorer + CrossEncoderScorer + GraphNeighborScorer + EntityExpansionScorer
    lifecycle.rs    — LifecyclePolicy + TtlExpirationPolicy
    consolidation.rs — ConsolidationStrategy + DedupConsolidation + CompactConsolidation + AutoRelateConsolidation
    ingestion.rs    — IngestionPipeline + EmbedAndExtractPipeline
    pipeline.rs     — SearchPipeline + WritePipeline + ConsolidationRunner + default_*() constructors
    compat.rs       — Blanket impls: MemoryStore → legacy traits
  memory_core/
    traits.rs       — (unchanged in Phase 1-3; deprecated in Phase 4)
    ...
```
