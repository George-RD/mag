use crate::memory_core::scoring::ScoringParams;
use crate::memory_core::{
    BackupInfo, CheckpointInput, GraphNode, ListResult, MemoryInput, MemoryUpdate, Relationship,
    SearchOptions, SearchResult, SemanticResult,
};
use crate::substrate::types::{
    CandidateSet, ConsolidationReport, QueryContext, ScoredCandidate, WriteContext,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════════
// 3.1 MemoryStore — Physical CRUD Backend
// ═══════════════════════════════════════════════════════════════════════════════

/// Physical CRUD + ancillary ops. Implemented by `SqliteStorage` today.
/// Every method mirrors an existing trait in `memory_core/traits.rs` so
/// blanket impls can delegate automatically (see §6).
#[async_trait]
pub trait MemoryStore: Send + Sync {
    // ── Core CRUD ─────────────────────────────────────────────────────────
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()>;

    /// Store a memory with a pre-computed embedding vector.
    ///
    /// Default implementation delegates to `store` (which may recompute the
    /// embedding). Backends that can accept pre-computed embeddings should
    /// override this to avoid double work.
    async fn store_with_embedding(
        &self,
        id: &str,
        data: &str,
        input: &MemoryInput,
        _embedding: Vec<f32>,
    ) -> Result<()> {
        self.store(id, data, input).await
    }

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
    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult>;

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
    async fn consolidate(&self, prune_days: i64, max_summaries: i64) -> Result<serde_json::Value>;
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

    async fn collect_vector_candidates(
        &self,
        query_embedding: &[f32],
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &ScoringParams,
    ) -> Result<CandidateSet>;

    async fn collect_fts_candidates(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &ScoringParams,
    ) -> Result<CandidateSet>;
    #[allow(clippy::too_many_arguments)]
    async fn enrich_graph_neighbors(
        &self,
        candidates: HashMap<String, ScoredCandidate>,
        query_tokens: &HashSet<String>,
        query_embedding: &[f32],
        limit: usize,
        include_superseded: bool,
        explain_enabled: bool,
        scoring_params: &ScoringParams,
    ) -> Result<HashMap<String, ScoredCandidate>>;
    #[allow(clippy::too_many_arguments)]
    async fn expand_entity_tags(
        &self,
        candidates: HashMap<String, ScoredCandidate>,
        query_tokens: &HashSet<String>,
        limit: usize,
        include_superseded: bool,
        explain_enabled: bool,
        scoring_params: &ScoringParams,
        opts: &SearchOptions,
    ) -> Result<HashMap<String, ScoredCandidate>>;

    // ── Welcome ───────────────────────────────────────────────────────────
    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3.2 RetrievalStrategy — Unscored Candidate Collection
// ═══════════════════════════════════════════════════════════════════════════════

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
    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet>;
}

/// Reference implementation: vector (embedding) similarity search.
///
/// Wraps `collect_vector_candidates` from `advanced.rs`.
/// Requires the `real-embeddings` or `sqlite-vec` feature.
pub struct VectorSearch {
    pub store: Arc<dyn MemoryStore>,
}

/// Reference implementation: BM25 full-text search.
///
/// Wraps `collect_fts_candidates` from `advanced.rs`.
pub struct FullTextSearch {
    pub store: Arc<dyn MemoryStore>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3.3 FusionStrategy — Multi-Signal Merging
// ═══════════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════════
// 3.4 Scorer — Composable Post-Fusion Scoring Chain
// ═══════════════════════════════════════════════════════════════════════════════

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
pub struct MultiFactorScorer;

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
    pub reranker: Arc<crate::memory_core::reranker::CrossEncoderReranker>,
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
    pub store: Arc<dyn MemoryStore>,
}

/// Reference: EntityExpansionScorer
///
/// Extracts entity tags from top seeds, queries memories sharing those tags,
/// injects them with ENTITY_EXPANSION_BOOST (1.15×).
/// Direct extraction of `expand_entity_candidates` (advanced.rs:678-866).
/// Caps expansion at 25 additional memories, 5 entity tags, 5 per tag.
pub struct EntityExpansionScorer {
    pub store: Arc<dyn MemoryStore>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3.5 LifecyclePolicy — TTL / Promotion / Decay
// ═══════════════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════════════
// 3.6 ConsolidationStrategy — Background Memory Restructuring
// ═══════════════════════════════════════════════════════════════════════════════

/// One self-contained consolidation pass over the memory store.
#[async_trait]
pub trait ConsolidationStrategy: Send + Sync {
    fn name(&self) -> &str;

    /// Run one consolidation pass.
    ///
    /// `dry_run = true` MUST produce the same `ConsolidationReport` but make
    /// no mutations to the store.
    async fn run(&self, store: &dyn MemoryStore, dry_run: bool) -> Result<ConsolidationReport>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3.7 IngestionPipeline — Write-Path Processing
// ═══════════════════════════════════════════════════════════════════════════════

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
    async fn ingest(&self, ctx: WriteContext, store: &dyn MemoryStore) -> Result<String>;
}
