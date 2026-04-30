//! Single-query pipeline runner used by the query-decomposition path in
//! `advanced_search`. Each sub-query (one per detected entity) is fed
//! through this function in parallel.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};

use super::super::conn_pool::ConnPool;
use super::super::query_classifier::{QueryIntent, classify_query_intent};
use super::fusion::fuse_and_score;
use super::retrieval::{collect_fts_candidates, collect_vector_candidates};
use crate::memory_core::embedder::Embedder;
use crate::memory_core::scoring_strategy::ScoringStrategy;
use crate::memory_core::{ScoringParams, SearchOptions, SemanticResult};

/// Run the core search pipeline for a single query: embed -> vector+FTS -> fuse -> refine.
///
/// Used by query decomposition to run each sub-query through the full pipeline.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_single_query_pipeline(
    pool: &Arc<ConnPool>,
    embedder: &Arc<dyn Embedder>,
    query: &str,
    candidate_limit: usize,
    limit: usize,
    opts: &SearchOptions,
    scoring_params: &ScoringParams,
    include_superseded: bool,
    explain_enabled: bool,
    scoring_strategy: &Arc<dyn ScoringStrategy>,
) -> Result<Vec<SemanticResult>> {
    let intent = classify_query_intent(query);
    // Route empty / whitespace-only queries through FTS-only — vector search
    // with an empty embedding can't produce meaningful similarities, and
    // `build_fts5_query` already short-circuits empty input safely.
    let fts_only = intent == QueryIntent::Keyword || query.trim().is_empty();

    let query_embedding = if fts_only {
        Vec::new()
    } else {
        let embedder = Arc::clone(embedder);
        let q = query.to_string();
        tokio::task::spawn_blocking(move || embedder.embed(&q))
            .await
            .context("spawn_blocking join error")??
    };

    let (vector_candidates, fts_candidates) = if fts_only {
        let pool = Arc::clone(pool);
        let q = query.to_string();
        let o = opts.clone();
        let sp = scoring_params.clone();
        let fts_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
        })
        .await
        .context("spawn_blocking join error")??;
        (Vec::new(), fts_result)
    } else if pool.has_readers() {
        let (vec_result, fts_result) = tokio::try_join!(
            tokio::task::spawn_blocking({
                let pool = Arc::clone(pool);
                let emb = query_embedding.clone();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_vector_candidates(
                        &conn,
                        &emb,
                        candidate_limit,
                        include_superseded,
                        &o,
                        &sp,
                    )
                }
            }),
            tokio::task::spawn_blocking({
                let pool = Arc::clone(pool);
                let q = query.to_string();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
                }
            }),
        )
        .context("parallel search join error")?;
        (vec_result?, fts_result?)
    } else {
        let pool = Arc::clone(pool);
        let emb = query_embedding.clone();
        let q = query.to_string();
        let o = opts.clone();
        let sp = scoring_params.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let vec_c = collect_vector_candidates(
                &conn,
                &emb,
                candidate_limit,
                include_superseded,
                &o,
                &sp,
            )?;
            let fts_c =
                collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)?;
            Ok::<_, anyhow::Error>((vec_c, fts_c))
        })
        .await
        .context("spawn_blocking join error")??
    };

    let ce_scores: Option<HashMap<String, f32>> = None;

    let pool_for_fuse = Arc::clone(pool);
    let q = query.to_string();
    let emb = query_embedding;
    let o = opts.clone();
    let sp = scoring_params.clone();
    let strat = Arc::clone(scoring_strategy);
    tokio::task::spawn_blocking(move || {
        let conn = pool_for_fuse.reader()?;
        fuse_and_score(
            &conn,
            vector_candidates,
            fts_candidates,
            &q,
            &emb,
            &o,
            limit,
            include_superseded,
            explain_enabled,
            &sp,
            ce_scores.as_ref(),
            strat.as_ref(),
        )
    })
    .await
    .context("spawn_blocking join error")?
}
