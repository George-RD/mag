use crate::substrate::traits::Scorer;
use crate::substrate::types::{QueryContext, ScoredCandidate};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════════
// GraphNeighborScorer
// ═══════════════════════════════════════════════════════════════════════════════

#[async_trait]
impl Scorer for crate::substrate::traits::GraphNeighborScorer {
    fn name(&self) -> &str {
        "graph-neighbor"
    }

    async fn score_batch(
        &self,
        candidates: &mut HashMap<String, ScoredCandidate>,
        ctx: &QueryContext,
    ) -> Result<()> {
        let tokens = ctx
            .query_tokens
            .clone()
            .unwrap_or_else(|| crate::memory_core::scoring::token_set(&ctx.query, 3));

        let taken: HashMap<String, ScoredCandidate> = candidates.clone();
        let updated = self
            .store
            .enrich_graph_neighbors(
                taken,
                &tokens,
                ctx.embedding_slice(),
                ctx.limit,
                ctx.include_superseded,
                ctx.opts.explain.unwrap_or(false),
                &ctx.scoring_params,
            )
            .await?;
        *candidates = updated;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EntityExpansionScorer
// ═══════════════════════════════════════════════════════════════════════════════

#[async_trait]
impl Scorer for crate::substrate::traits::EntityExpansionScorer {
    fn name(&self) -> &str {
        "entity-expansion"
    }

    async fn score_batch(
        &self,
        candidates: &mut HashMap<String, ScoredCandidate>,
        ctx: &QueryContext,
    ) -> Result<()> {
        let tokens = ctx
            .query_tokens
            .clone()
            .unwrap_or_else(|| crate::memory_core::scoring::token_set(&ctx.query, 3));

        let taken: HashMap<String, ScoredCandidate> = candidates.clone();
        let updated = self
            .store
            .expand_entity_tags(
                taken,
                &tokens,
                ctx.limit,
                ctx.include_superseded,
                ctx.opts.explain.unwrap_or(false),
                &ctx.scoring_params,
                &ctx.opts,
            )
            .await?;
        *candidates = updated;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CrossEncoderScorer
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "real-embeddings")]
#[async_trait]
impl Scorer for crate::substrate::traits::CrossEncoderScorer {
    fn name(&self) -> &str {
        "cross-encoder"
    }

    async fn score_batch(
        &self,
        candidates: &mut HashMap<String, ScoredCandidate>,
        ctx: &QueryContext,
    ) -> Result<()> {
        let top_n = ctx.scoring_params.rerank_top_n;
        let alpha = ctx.scoring_params.rerank_blend_alpha;

        if candidates.is_empty() || top_n == 0 {
            return Ok(());
        }

        // Collect and sort candidates by score descending, take top N
        let mut sorted: Vec<(&String, &ScoredCandidate)> = candidates.iter().collect();
        sorted.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let top: Vec<(&String, &ScoredCandidate)> = sorted.into_iter().take(top_n).collect();

        let ids: Vec<String> = top.iter().map(|(id, _)| (*id).clone()).collect();
        let passages: Vec<String> = top.iter().map(|(_, c)| c.result.content.clone()).collect();

        let reranker = Arc::clone(&self.reranker);
        let query = ctx.query.clone();

        let ce_scores = tokio::task::spawn_blocking(move || {
            let passage_refs: Vec<&str> = passages.iter().map(|s| s.as_str()).collect();
            reranker.score_batch(&query, &passage_refs)
        })
        .await
        .context("spawn_blocking join error")??;

        for (id, ce_score) in ids.iter().zip(ce_scores.iter()) {
            if let Some(candidate) = candidates.get_mut(id) {
                candidate.score = alpha * candidate.score + (1.0 - alpha) * f64::from(*ce_score);
            }
        }

        Ok(())
    }
}
