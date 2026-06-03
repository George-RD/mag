use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::memory_core::storage::sqlite::pipeline::scoring::refine_scores;
use crate::substrate::traits::{MultiFactorScorer, Scorer};
use crate::substrate::types::{QueryContext, ScoredCandidate};

#[async_trait]
impl Scorer for MultiFactorScorer {
    fn name(&self) -> &str {
        "multi_factor"
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

        refine_scores(
            candidates,
            &tokens,
            &ctx.opts,
            ctx.opts.explain.unwrap_or(false),
            &ctx.scoring_params,
        );

        Ok(())
    }
}
