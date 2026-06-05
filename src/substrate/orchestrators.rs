use std::collections::HashMap;
use std::sync::Arc;

use crate::memory_core::{MemoryInput, SemanticResult};
use crate::substrate::traits::{
    ConsolidationStrategy, FusionStrategy, IngestionPipeline, LifecyclePolicy, MemoryStore,
    RetrievalStrategy, Scorer,
};
use crate::substrate::types::{
    CandidateSet, ConsolidationReport, QueryContext, ScoredCandidate, WriteContext,
};

use anyhow::Result;
use futures::future::join_all;
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
    #[allow(clippy::cast_possible_truncation)]
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
        // Step 1: Run all retrieval strategies concurrently.
        let mut candidate_sets: HashMap<&str, CandidateSet> =
            HashMap::with_capacity(self.retrieval.len());
        match self.retrieval.len() {
            0 => {}
            1 => {
                let result = self.retrieval[0].collect(&ctx).await;
                candidate_sets.insert(self.retrieval[0].name(), result?);
            }
            2 => {
                let (a, b) = tokio::try_join!(
                    self.retrieval[0].collect(&ctx),
                    self.retrieval[1].collect(&ctx),
                )?;
                candidate_sets.insert(self.retrieval[0].name(), a);
                candidate_sets.insert(self.retrieval[1].name(), b);
            }
            _ => {
                let futures: Vec<_> = self.retrieval.iter().map(|s| s.collect(&ctx)).collect();
                let sets = join_all(futures).await;
                for (strategy, result) in self.retrieval.iter().zip(sets) {
                    candidate_sets.insert(strategy.name(), result?);
                }
            }
        }

        // Step 2: Fuse candidates into a single ranked list.
        let fused = self.fusion.fuse(candidate_sets, &ctx.scoring_params);

        // Step 3: Convert to HashMap for scorer chain.
        let mut candidates: HashMap<String, ScoredCandidate> = fused
            .into_iter()
            .map(|c| (c.result.id.clone(), c))
            .collect();

        // Step 4: Apply each scorer in order.
        for scorer in &self.scorers {
            scorer.score_batch(&mut candidates, &ctx).await?;
        }

        // Step 5: Apply lifecycle filter if set.
        let mut alive: Vec<ScoredCandidate> = if let Some(ref lifecycle) = self.lifecycle {
            candidates
                .into_values()
                .filter(|c| lifecycle.is_alive(c))
                .collect()
        } else {
            candidates.into_values().collect()
        };

        // Step 6: Abstention gate.
        if !alive.is_empty() {
            let max_text_overlap = alive.iter().map(|c| c.text_overlap).fold(0.0f64, f64::max);
            if max_text_overlap < self.abstention_min_text {
                return Ok(Vec::new());
            }
        }

        // Step 7: Sort descending by score, truncate to limit.
        alive.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
        let max_score = alive.first().map(|c| c.score).unwrap_or(0.0);
        let limit = ctx.limit;
        let explain_enabled = ctx.opts.explain.unwrap_or(false);

        let results: Vec<SemanticResult> = alive
            .into_iter()
            .take(limit)
            .map(|mut candidate| {
                let normalized = if max_score > 0.0 {
                    (candidate.score / max_score).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                candidate.result.score = normalized as f32;
                if explain_enabled {
                    candidate.result.metadata = {
                        let mut meta = candidate.result.metadata.clone();
                        if let Some(ref explain) = candidate.explain
                            && let Some(obj) = meta.as_object_mut()
                        {
                            obj.insert("explain".to_string(), explain.clone());
                        }
                        meta
                    };
                }
                candidate.result
            })
            .collect();

        Ok(results)
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
