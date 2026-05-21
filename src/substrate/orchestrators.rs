use std::collections::HashMap;
use std::sync::Arc;

use crate::memory_core::{MemoryInput, SemanticResult};
use crate::substrate::traits::{
    ConsolidationStrategy, FusionStrategy, IngestionPipeline, LifecyclePolicy, MemoryStore,
    RetrievalStrategy, Scorer,
};
use crate::substrate::types::{CandidateSet, ConsolidationReport, QueryContext, WriteContext};

use anyhow::Result;
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
        let mut candidates: HashMap<&str, CandidateSet> =
            HashMap::with_capacity(self.retrieval.len());
        for strategy in &self.retrieval {
            let set = strategy.collect(&ctx).await?;
            candidates.insert(strategy.name(), set);
        }
        let _fused = self.fusion.fuse(candidates, &ctx.scoring_params);
        todo!("Phase 3")
    }
}

/// The write-path orchestrator. Thin wrapper around `IngestionPipeline`.
pub struct WritePipeline {
    pub pipeline: Box<dyn IngestionPipeline>,
    pub store: Arc<dyn MemoryStore>,
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

/// Runs consolidation strategies in registration order.
pub struct ConsolidationRunner {
    pub strategies: Vec<Box<dyn ConsolidationStrategy>>,
    pub store: Arc<dyn MemoryStore>,
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
