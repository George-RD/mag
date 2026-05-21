use crate::memory_core::scoring::ScoringParams;
use crate::memory_core::{MemoryInput, SearchOptions, SemanticResult};
use std::collections::HashSet;

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
    pub query_tokens: Option<HashSet<String>>,
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
