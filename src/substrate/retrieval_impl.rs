use crate::substrate::traits::{FullTextSearch, RetrievalStrategy, VectorSearch};
use anyhow::{Result, anyhow};
use async_trait::async_trait;

use crate::substrate::types::{CandidateSet, QueryContext};

#[async_trait]
impl RetrievalStrategy for VectorSearch {
    fn name(&self) -> &str {
        "vector"
    }

    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet> {
        if ctx.query_embedding.is_none() {
            return Err(anyhow!("query_embedding not populated"));
        }
        self.store
            .collect_vector_candidates(
                ctx.embedding_slice(),
                ctx.limit,
                &ctx.opts,
                ctx.include_superseded,
                &ctx.scoring_params,
            )
            .await
    }
}

#[async_trait]
impl RetrievalStrategy for FullTextSearch {
    fn name(&self) -> &str {
        "fts"
    }

    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet> {
        self.store
            .collect_fts_candidates(
                &ctx.query,
                ctx.limit,
                &ctx.opts,
                ctx.include_superseded,
                &ctx.scoring_params,
            )
            .await
    }
}
