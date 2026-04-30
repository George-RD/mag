//! Phase 3: RRF fusion + dual-match boost + cross-encoder blend.
//!
//! Operates on the (vector, FTS) candidate pair from [`super::retrieval`] and
//! produces a `HashMap<String, RankedSemanticCandidate>` ranked by combined
//! score. The Phase 5 (graph enrichment) and Phase 6 (abstention/dedup) steps
//! are dispatched downstream by the caller.
//!
//! NOTE: `fuse_refine_and_output` was originally a single function that
//! covered phases 3-6. It is split here as `fuse_and_score` (phases 3, 3b,
//! 4, 5, and 5b — i.e., everything that mutates the ranked map in place)
//! and [`super::abstention::abstain_and_dedup`] (phase 6 dedup, abstention,
//! strategy pass, and final output). The boundary is the `// ── Phase 6:`
//! comment that previously lived inside `fuse_refine_and_output`.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;

use super::super::storage::RankedSemanticCandidate;
use super::abstention::abstain_and_dedup;
use super::enrichment::{enrich_graph_neighbors, expand_entity_tags};
use super::scoring::refine_scores;
use crate::memory_core::scoring_strategy::ScoringStrategy;
use crate::memory_core::{
    EventType, ScoringParams, SearchOptions, SemanticResult, priority_factor, token_set,
    type_weight_et,
};

/// Phases 3-6: RRF fusion, score refinement, graph enrichment, abstention.
///
/// This is the orchestrator that the residual `advanced.rs` calls into. It
/// internally invokes [`enrich_graph_neighbors`], [`expand_entity_tags`],
/// [`refine_scores`], and finally [`abstain_and_dedup`] for the Phase 6
/// dedup/abstention/output stage.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fuse_and_score(
    conn: &Connection,
    vector_candidates: Vec<(String, f64, RankedSemanticCandidate)>,
    fts_candidates: Vec<(String, f64, RankedSemanticCandidate)>,
    query: &str,
    query_embedding: &[f32],
    opts: &SearchOptions,
    limit: usize,
    include_superseded: bool,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
    cross_encoder_scores: Option<&HashMap<String, f32>>,
    scoring_strategy: &dyn ScoringStrategy,
) -> Result<Vec<SemanticResult>> {
    // Phase 3: Weighted RRF fusion -- vector similarity weighted higher
    // for semantic discrimination (Oracle recommendation)
    let mut ranked: HashMap<String, RankedSemanticCandidate> = HashMap::new();

    // Track which IDs appear in FTS results for dual_match detection
    let fts_ids: HashSet<String> = if explain_enabled {
        fts_candidates.iter().map(|(id, _, _)| id.clone()).collect()
    } else {
        HashSet::new()
    };

    for (rank, (id, _sim, mut candidate)) in vector_candidates.into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let rrf_score = scoring_params.rrf_weight_vec / (scoring_params.rrf_k + rank as f64 + 1.0);
        let et_ref = candidate
            .result
            .event_type
            .as_ref()
            .unwrap_or(&EventType::Memory);
        let type_w = type_weight_et(et_ref);
        let pf = priority_factor(candidate.priority_value, scoring_params);

        if explain_enabled {
            let dual = fts_ids.contains(&id);
            candidate.explain = Some(serde_json::json!({
                "vec_sim": candidate.vec_sim,
                "fts_rank": null,
                "rrf_score": rrf_score,
                "dual_match": dual,
                "type_weight": type_w,
                "priority_factor": pf,
            }));
        }

        candidate.score *= rrf_score;
        ranked.insert(id, candidate);
    }
    let mut dual_match_ids: HashMap<String, usize> = HashMap::new();
    for (rank, (id, bm25_raw, candidate)) in fts_candidates.into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let rrf_score = scoring_params.rrf_weight_fts / (scoring_params.rrf_k + rank as f64 + 1.0);
        if let Some(existing) = ranked.get_mut(&id) {
            // Present in both -- add the FTS RRF contribution
            existing.score += candidate.score * rrf_score;
            dual_match_ids.insert(id, rank);
            if explain_enabled && let Some(ref mut exp) = existing.explain {
                exp["fts_rank"] = serde_json::json!(rank);
                exp["fts_bm25"] = serde_json::json!(bm25_raw);
                exp["dual_match"] = serde_json::json!(true);
                // Update rrf_score to show the combined contribution
                let vec_rrf = exp["rrf_score"].as_f64().unwrap_or(0.0);
                exp["rrf_score"] = serde_json::json!(vec_rrf + rrf_score);
            }
        } else {
            let et_ref = candidate
                .result
                .event_type
                .as_ref()
                .unwrap_or(&EventType::Memory);
            let type_w = type_weight_et(et_ref);
            let pf = priority_factor(candidate.priority_value, scoring_params);

            let explain_data = if explain_enabled {
                Some(serde_json::json!({
                    "vec_sim": null,
                    "fts_rank": rank,
                    "fts_bm25": bm25_raw,
                    "rrf_score": rrf_score,
                    "dual_match": false,
                    "type_weight": type_w,
                    "priority_factor": pf,
                }))
            } else {
                None
            };

            let mut merged = candidate;
            merged.score *= rrf_score;
            merged.explain = explain_data;
            ranked.insert(id, merged);
        }
    }
    // Apply adaptive dual-match boost: candidates in both vector and FTS
    // lists get a multiplicative boost scaled by their FTS rank position.
    // text_rel uses inverse-rank decay: 1/(1+rank), giving a soft falloff
    // (rank 0 → 1.0, rank 1 → 0.5, rank 5 → 0.17, …).  The adaptive
    // boost ranges from 1.3x (low text relevance) to 1.8x (top FTS match).
    if !dual_match_ids.is_empty() {
        for (id, fts_rank) in &dual_match_ids {
            if let Some(candidate) = ranked.get_mut(id) {
                #[allow(clippy::cast_precision_loss)]
                let text_rel = 1.0 / (1.0 + *fts_rank as f64);
                let base = scoring_params.dual_match_boost.max(1.0);
                let adaptive_boost = base + text_rel * 0.5;
                candidate.score *= adaptive_boost;
                if explain_enabled && let Some(ref mut exp) = candidate.explain {
                    exp["adaptive_dual_boost"] = serde_json::json!(adaptive_boost);
                    exp["fts_text_rel"] = serde_json::json!(text_rel);
                }
            }
        }
    }

    // ── Phase 3b: Cross-encoder reranking blend ──────────────────────
    if let Some(ce_scores) = cross_encoder_scores {
        let alpha = scoring_params.rerank_blend_alpha;
        for (id, candidate) in ranked.iter_mut() {
            if let Some(&ce_score) = ce_scores.get(id) {
                let rrf_score = candidate.score;
                candidate.score = alpha * rrf_score + (1.0 - alpha) * ce_score as f64;
                if explain_enabled && let Some(ref mut exp) = candidate.explain {
                    exp["cross_encoder_score"] = serde_json::json!(ce_score);
                    exp["rerank_blend_alpha"] = serde_json::json!(alpha);
                }
            }
        }
    }

    let query_tokens = token_set(query, 3);
    refine_scores(
        &mut ranked,
        &query_tokens,
        opts,
        explain_enabled,
        scoring_params,
    );

    // ── Phase 5: Graph enrichment ──
    enrich_graph_neighbors(
        conn,
        &mut ranked,
        &query_tokens,
        query_embedding,
        limit,
        include_superseded,
        explain_enabled,
        scoring_params,
    );

    // ── Phase 5b: Entity expansion ──
    expand_entity_tags(
        conn,
        &mut ranked,
        &query_tokens,
        limit,
        include_superseded,
        explain_enabled,
        scoring_params,
        opts,
    );

    // ── Phase 6: Collection-level abstention + dedup ─────────────
    abstain_and_dedup(
        ranked,
        &query_tokens,
        opts,
        limit,
        explain_enabled,
        scoring_params,
        scoring_strategy,
        query,
    )
}
