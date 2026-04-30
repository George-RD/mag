//! Phase 4: Score refinement — word overlap, coverage boost, Jaccard,
//! feedback, time decay, importance, and context tag matching.
//!
//! Also hosts the keyword-only result conversion path used by
//! `KeywordOnlyStrategy` dispatch in `advanced_search`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};

use super::super::storage::{RankedSemanticCandidate, SqliteStorage};
use super::abstention::merge_hot_cache_results;
use crate::memory_core::retrieval_strategy::{CandidateSet, FtsSearcher};
use crate::memory_core::scoring::query_coverage_boost;
use crate::memory_core::scoring_strategy::ScoringStrategy;
use crate::memory_core::{
    EventType, ScoringParams, SearchOptions, SemanticResult, feedback_factor, jaccard_pre,
    time_decay_et, token_set, word_overlap_pre,
};

/// Phase 4: Score refinement — word overlap, coverage boost, Jaccard, feedback,
/// time decay, importance, and context tag matching.
pub(crate) fn refine_scores(
    ranked: &mut HashMap<String, RankedSemanticCandidate>,
    query_tokens: &HashSet<String>,
    opts: &SearchOptions,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
) {
    for candidate in ranked.values_mut() {
        let with_tags = if candidate.result.tags.is_empty() {
            candidate.result.content.clone()
        } else {
            format!(
                "{} {}",
                candidate.result.content,
                candidate.result.tags.join(" ")
            )
        };
        let candidate_tokens = token_set(&with_tags, 3);
        let overlap = word_overlap_pre(query_tokens, &candidate_tokens);
        candidate.text_overlap = overlap;
        let fb_score = candidate
            .result
            .metadata
            .get("feedback_score")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let fb_dampening = if fb_score < 0 { 0.5 } else { 1.0 };
        let coverage_boost =
            1.0 + (query_coverage_boost(overlap, scoring_params) - 1.0) * fb_dampening;
        candidate.score *= coverage_boost;
        candidate.score *= 1.0 + overlap * scoring_params.word_overlap_weight * fb_dampening;
        let jaccard = jaccard_pre(query_tokens, &candidate_tokens);
        candidate.score *= 1.0 + jaccard * scoring_params.jaccard_weight;
        let fb_factor = feedback_factor(fb_score, scoring_params);
        candidate.score *= fb_factor;
        let et_ref = candidate
            .result
            .event_type
            .as_ref()
            .unwrap_or(&EventType::Memory);
        let td = time_decay_et(&candidate.created_at, et_ref, scoring_params);
        candidate.score *= td;
        let importance_factor_val = scoring_params.importance_floor
            + candidate.result.importance * scoring_params.importance_scale;
        candidate.score *= importance_factor_val;

        if let Some(context_tags) = opts.context_tags.as_ref() {
            let candidate_tags: HashSet<String> = candidate
                .result
                .tags
                .iter()
                .map(|t| t.to_lowercase())
                .collect();
            let context_norm: Vec<String> = context_tags
                .iter()
                .map(|t| t.to_lowercase())
                .filter(|t| !t.is_empty())
                .collect();
            if !context_norm.is_empty() {
                let matched = context_norm
                    .iter()
                    .filter(|t| candidate_tags.contains(*t))
                    .count();
                #[allow(clippy::cast_precision_loss)]
                let ratio = matched as f64 / context_norm.len() as f64;
                candidate.score *= 1.0 + ratio * scoring_params.context_tag_weight;
            }
        }

        if explain_enabled && let Some(ref mut exp) = candidate.explain {
            exp["word_overlap"] = serde_json::json!(overlap);
            exp["query_coverage_boost"] = serde_json::json!(coverage_boost);
            exp["text_overlap"] = serde_json::json!(overlap);
            exp["importance_factor"] = serde_json::json!(importance_factor_val);
            exp["feedback_factor"] = serde_json::json!(fb_factor);
            exp["time_decay"] = serde_json::json!(td);
        }
    }
}

/// Convert a `CandidateSet` from `KeywordOnlyStrategy` into `Vec<SemanticResult>`.
///
/// Applies word-overlap and time-decay scoring to produce final scores.
/// Applies the abstention gate: if the best text-overlap across all candidates
/// is below `scoring_params.abstention_min_text`, returns an empty vec (same
/// behaviour as the full pipeline's abstention gate).
/// Uses `ScoringStrategy` for the final score computation. Candidates are
/// sorted by final score descending and truncated to `limit`.
///
/// This function runs synchronously and is intended for use inside
/// `spawn_blocking`.
pub(crate) fn keyword_candidates_to_results(
    candidates: CandidateSet,
    query: &str,
    limit: usize,
    scoring_params: &ScoringParams,
    scoring_strategy: &dyn ScoringStrategy,
    explain_enabled: bool,
) -> Vec<SemanticResult> {
    let query_tokens = token_set(query, 3);

    // Score all candidates, keeping them as RankedSemanticCandidate so we
    // can read text_overlap for the abstention gate before converting.
    let mut ranked: Vec<(f64, RankedSemanticCandidate)> = candidates
        .into_iter()
        .map(|(_id, bm25_raw, mut candidate)| {
            // Compute word overlap for keyword scoring.
            let content_tokens = token_set(&candidate.result.content, 3);
            let overlap = word_overlap_pre(&query_tokens, &content_tokens);
            candidate.text_overlap = overlap;

            // Build a keyword-specific score: base score (from type weight
            // and priority, already set during FTS collection) boosted by
            // word overlap and importance.
            let base = candidate.score; // type_weight * priority_factor
            let overlap_boost = overlap * scoring_params.word_overlap_weight;
            let importance_factor = scoring_params.importance_floor
                + candidate.result.importance * scoring_params.importance_scale;
            let et = candidate
                .result
                .event_type
                .as_ref()
                .unwrap_or(&EventType::Memory);
            let time_decay = time_decay_et(&candidate.created_at, et, scoring_params);
            let keyword_score = base * (1.0 + overlap_boost) * importance_factor * time_decay;
            candidate.score = keyword_score;

            // Let the scoring strategy have the final say.
            let final_score = scoring_strategy.score(&candidate, query, scoring_params);
            candidate.score = final_score;

            if explain_enabled {
                candidate.explain = Some(serde_json::json!({
                    "strategy": "keyword-only",
                    "bm25_raw": bm25_raw,
                    "word_overlap": overlap,
                    "base_score": base,
                    "importance_factor": importance_factor,
                    "time_decay": time_decay,
                    "final_score": final_score,
                    "skipped_phases": [
                        "vector_search",
                        "rrf_fusion",
                        "cross_encoder",
                        "graph_enrichment",
                        "entity_expansion",
                    ],
                }));
            }

            (final_score, candidate)
        })
        .collect();

    // Abstention gate: mirror the full pipeline's behaviour. If no candidate
    // has enough text overlap with the query, treat the result set as a miss.
    if !query_tokens.is_empty() {
        let max_text_overlap = ranked
            .iter()
            .map(|(_, c)| c.text_overlap)
            .fold(0.0f64, f64::max);
        if max_text_overlap < scoring_params.abstention_min_text {
            return Vec::new();
        }
    }

    ranked.sort_by(|(a, _), (b, _)| b.total_cmp(a));
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(final_score, mut candidate)| {
            #[allow(clippy::cast_possible_truncation)]
            {
                candidate.result.score = final_score as f32;
            }
            candidate.result
        })
        .collect()
}

/// `KeywordOnlyStrategy` dispatch path for `advanced_search`.
///
/// For keyword-intent queries (and empty / whitespace-only queries, which
/// can't produce a meaningful embedding), this skips embedding, vector
/// search, RRF fusion, reranker, and graph enrichment, using FTS5 BM25
/// only. Merges hot-cache hits when the cache had a confident text match.
pub(crate) async fn run_keyword_only_search(
    storage: &SqliteStorage,
    query: &str,
    limit: usize,
    opts: &SearchOptions,
    scoring_params: &ScoringParams,
    hot_results: Vec<SemanticResult>,
    hot_has_confident_match: bool,
) -> Result<Vec<SemanticResult>> {
    let include_superseded = opts.include_superseded.unwrap_or(false);
    let explain_enabled = opts.explain.unwrap_or(false);
    let candidates: CandidateSet = storage
        .fts_search(query, limit, opts, include_superseded, scoring_params)
        .await?;

    let scoring_strategy = Arc::clone(&storage.scoring_strategy);
    let query_owned = query.to_string();
    let sp = scoring_params.clone();
    let results = tokio::task::spawn_blocking(move || {
        keyword_candidates_to_results(
            candidates,
            &query_owned,
            limit,
            &sp,
            scoring_strategy.as_ref(),
            explain_enabled,
        )
    })
    .await
    .context("spawn_blocking join error")?;

    let results = if hot_has_confident_match {
        merge_hot_cache_results(hot_results, results, limit)
    } else {
        results
    };

    Ok(results)
}
