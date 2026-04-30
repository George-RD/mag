//! Single-query pipeline runner used by the query-decomposition path in
//! `advanced_search`. Each sub-query (one per detected entity) is fed
//! through this function in parallel.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};

use super::super::conn_pool::ConnPool;
use super::super::nlp::{
    content_fingerprint, extract_query_entities, extract_topic_keywords, generate_sub_queries,
};
use super::super::query_classifier::{QueryIntent, classify_query_intent};
use super::fusion::fuse_and_score;
use super::retrieval::{collect_dual_candidates, collect_fts_candidates};
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
        let pool_arc = Arc::clone(pool);
        let q = query.to_string();
        let o = opts.clone();
        let sp = scoring_params.clone();
        let fts_result = tokio::task::spawn_blocking(move || {
            let conn = pool_arc.reader()?;
            collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
        })
        .await
        .context("spawn_blocking join error")??;
        (Vec::new(), fts_result)
    } else {
        collect_dual_candidates(
            pool,
            query,
            &query_embedding,
            candidate_limit,
            include_superseded,
            opts,
            scoring_params,
        )
        .await?
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

/// Enrich a base result set by running each detected sub-query through the
/// single-query pipeline in parallel and merging the results.
///
/// For multi-entity queries we extract entities/topics, generate sub-queries,
/// then fan out the remaining sub-queries (skipping the first, which is the
/// original query already represented by `base_results`). Sub-results are
/// merged: new ids appended; on duplicate id, the higher score wins (first
/// occurrence wins on ties for stable ordering across parallel completion);
/// then the final list is fingerprint-deduped on content before truncating
/// to `limit`.
///
/// If decomposition does not apply (fewer than two entities, no topics, or
/// only one sub-query) the base results are returned unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn enrich_with_decomposition(
    pool: &Arc<ConnPool>,
    embedder: &Arc<dyn Embedder>,
    scoring_strategy: &Arc<dyn ScoringStrategy>,
    base_results: Vec<SemanticResult>,
    query_for_decomp: &str,
    candidate_limit: usize,
    limit: usize,
    opts: &SearchOptions,
    scoring_params: &ScoringParams,
    include_superseded: bool,
    explain_enabled: bool,
) -> Result<Vec<SemanticResult>> {
    let decomp_entities = extract_query_entities(query_for_decomp);
    if decomp_entities.len() < 2 {
        return Ok(base_results);
    }
    let topics = extract_topic_keywords(query_for_decomp, &decomp_entities);
    let sub_queries = generate_sub_queries(query_for_decomp, &decomp_entities, &topics);
    if topics.is_empty() || sub_queries.len() <= 1 {
        return Ok(base_results);
    }

    let mut all_results = base_results;
    let mut seen_ids: HashSet<String> = all_results.iter().map(|r| r.id.clone()).collect();

    // Parallel sub-query execution (resolves #121).
    // ConnPool has 4 dedicated reader connections in WAL mode.
    // Each sub-query internally runs vector + FTS in try_join!,
    // consuming 2 readers simultaneously, so effective parallelism
    // is ~2 sub-queries at a time; additional queries queue on the
    // reader mutexes without deadlock.  Results are collected with
    // their original index and sorted before merging to preserve
    // deterministic dedup ordering.
    let mut join_set: tokio::task::JoinSet<(usize, Result<Vec<SemanticResult>>)> =
        tokio::task::JoinSet::new();
    for (idx, sub_query) in sub_queries.iter().skip(1).enumerate() {
        let pool = Arc::clone(pool);
        let embedder = Arc::clone(embedder);
        let sq = sub_query.clone();
        let opts = opts.clone();
        let sp = scoring_params.clone();
        let strat = Arc::clone(scoring_strategy);
        join_set.spawn(async move {
            let res = run_single_query_pipeline(
                &pool,
                &embedder,
                &sq,
                candidate_limit,
                limit,
                &opts,
                &sp,
                include_superseded,
                explain_enabled,
                &strat,
            )
            .await;
            (idx, res)
        });
    }
    // Collect all results, then sort by original sub-query index
    // so merge order is deterministic (same as the old sequential loop).
    let mut indexed_results: Vec<(usize, Vec<SemanticResult>)> = Vec::new();
    while let Some(task_result) = join_set.join_next().await {
        let (idx, sub_results) = task_result.context("sub-query task panicked")?;
        indexed_results.push((idx, sub_results?));
    }
    indexed_results.sort_by_key(|(idx, _)| *idx);
    for (_idx, sub_results) in indexed_results {
        for result in sub_results {
            if seen_ids.insert(result.id.clone()) {
                all_results.push(result);
            } else if let Some(existing) = all_results.iter_mut().find(|r| r.id == result.id)
                && result.score > existing.score
            {
                existing.score = result.score;
            }
        }
    }

    let mut deduped: Vec<SemanticResult> = Vec::new();
    let mut fingerprints: HashSet<String> = HashSet::new();
    all_results.sort_by(|a, b| b.score.total_cmp(&a.score));
    for result in all_results {
        let fp = content_fingerprint(&result.content);
        if fingerprints.insert(fp) {
            deduped.push(result);
        }
    }
    deduped.truncate(limit);
    Ok(deduped)
}
