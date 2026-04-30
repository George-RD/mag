//! Phase 2.5: Cross-encoder reranking blend.
//!
//! Computes pairwise (query, candidate) scores for the top candidates if a
//! reranker is configured. Returned scores feed into the Phase 3 RRF blend
//! in [`super::fusion`].

use std::collections::{HashMap, HashSet};

use super::super::storage::RankedSemanticCandidate;
use crate::memory_core::ScoringParams;

/// Compute cross-encoder scores for the top candidates if a reranker is available.
///
/// Returns `None` when no reranker is configured or the candidate list is empty.
/// Must be called from inside a `spawn_blocking` closure — `rerank` is synchronous
/// and may block on ONNX inference.
pub(super) fn compute_cross_encoder_scores(
    reranker: Option<&std::sync::Arc<dyn crate::memory_core::reranker::Reranker>>,
    query: &str,
    vector_candidates: &[(String, f64, RankedSemanticCandidate)],
    fts_candidates: &[(String, f64, RankedSemanticCandidate)],
    scoring_params: &ScoringParams,
) -> Option<HashMap<String, f32>> {
    let reranker = reranker?;
    let mut seen: HashSet<&str> = HashSet::with_capacity(scoring_params.rerank_top_n);
    let mut candidates_for_rerank: Vec<(&str, &str)> =
        Vec::with_capacity(scoring_params.rerank_top_n);
    // Interleave vector and FTS candidates to avoid biasing towards either source
    // when truncating to rerank_top_n.
    let max_idx = vector_candidates.len().max(fts_candidates.len());
    for i in 0..max_idx {
        if candidates_for_rerank.len() >= scoring_params.rerank_top_n {
            break;
        }
        if let Some((id, _, candidate)) = vector_candidates.get(i)
            && seen.insert(id.as_str())
        {
            candidates_for_rerank.push((id.as_str(), &candidate.result.content));
        }
        if candidates_for_rerank.len() >= scoring_params.rerank_top_n {
            break;
        }
        if let Some((id, _, candidate)) = fts_candidates.get(i)
            && seen.insert(id.as_str())
        {
            candidates_for_rerank.push((id.as_str(), &candidate.result.content));
        }
    }
    if candidates_for_rerank.is_empty() {
        return None;
    }
    match reranker.rerank(query, &candidates_for_rerank) {
        Ok(map) => {
            if map.is_empty() {
                None
            } else {
                Some(map)
            }
        }
        Err(e) => {
            tracing::warn!("cross-encoder reranking failed, skipping: {e}");
            None
        }
    }
}
