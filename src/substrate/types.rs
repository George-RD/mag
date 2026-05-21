use crate::memory_core::scoring::ScoringParams;
use crate::memory_core::{MemoryInput, SearchOptions};
use std::collections::HashSet;

pub use crate::memory_core::storage::sqlite::RankedSemanticCandidate as ScoredCandidate;

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
impl QueryContext {
    /// View the query embedding as a slice, returning an empty slice when
    /// no embedding is set. Convenience for helpers that take `&[f32]`.
    #[must_use]
    pub fn embedding_slice(&self) -> &[f32] {
        self.query_embedding.as_deref().unwrap_or(&[])
    }
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
