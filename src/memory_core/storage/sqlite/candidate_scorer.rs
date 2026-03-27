//! Centralised scoring gateway for the SQLite storage layer.
//!
//! **All** scoring functions used anywhere under `storage::sqlite` should be
//! imported from this module rather than reaching into `crate::memory_core`
//! directly.  This keeps the dependency surface in one place and makes it
//! easy to swap or override scoring logic later.
//!
//! The module provides:
//! - **Re-exports** of low-level primitives (`token_set`, `jaccard_pre`, etc.)
//! - **Composite helpers** (`score_initial_candidate`, `refine_candidate_score`,
//!   etc.) that encapsulate multi-step scoring computations.

use std::collections::HashSet;

use crate::memory_core::scoring::{ENTITY_EXPANSION_BOOST, query_coverage_boost};

// ── Re-exports for sibling modules ──────────────────────────────────────────
// Siblings (hot_cache, crud, helpers, advanced, mod) import scoring primitives
// from here instead of reaching into `crate::memory_core` directly.

pub(super) use crate::memory_core::ScoringParams;
pub(super) use crate::memory_core::{
    EventType, feedback_factor, is_stopword, jaccard_pre, jaccard_similarity, priority_factor,
    simple_stem, time_decay_et, token_set, type_weight_et, word_overlap_pre,
};

/// Compute the initial candidate score from event type and priority.
///
/// Used during Phase 1 (vector candidate collection) and Phase 2 (FTS
/// candidate collection) to assign a baseline score before text-based
/// reranking.
#[inline]
pub(super) fn score_initial_candidate(
    et: &EventType,
    priority_value: u8,
    scoring_params: &ScoringParams,
) -> f64 {
    type_weight_et(et) * priority_factor(priority_value, scoring_params)
}

/// Result of text-based refinement scoring (Phase 4 in the pipeline).
///
/// Returned by [`refine_candidate_score`] so the caller can apply the
/// multiplier and record explain-mode metadata without duplicating logic.
pub(super) struct RefinementResult {
    /// Multiplicative factor to apply to the candidate's current score.
    pub factor: f64,
    /// Word overlap ratio between query and candidate tokens.
    pub text_overlap: f64,
    /// The coverage boost component (for explain metadata).
    pub coverage_boost: f64,
    /// The importance factor component (for explain metadata).
    pub importance_factor: f64,
    /// The feedback factor component (for explain metadata).
    pub feedback_factor: f64,
    /// The time decay component (for explain metadata).
    pub time_decay: f64,
}

/// Compute the text-based refinement score for a candidate.
///
/// This encapsulates the full Phase 4 refinement: word overlap, query
/// coverage boost, Jaccard similarity, feedback factor, time decay, and
/// importance scaling.  Context tag boosting is intentionally left in
/// `advanced.rs` because it depends on `SearchOptions`.
///
/// Returns a [`RefinementResult`] containing a multiplicative factor and
/// individual component values for explain-mode metadata.
#[allow(clippy::too_many_arguments)]
pub(super) fn refine_candidate_score(
    query_tokens: &HashSet<String>,
    content: &str,
    tags: &[String],
    importance: f64,
    feedback_score: i64,
    created_at: &str,
    et: &EventType,
    scoring_params: &ScoringParams,
) -> RefinementResult {
    let with_tags = if tags.is_empty() {
        content.to_string()
    } else {
        format!("{} {}", content, tags.join(" "))
    };
    let candidate_tokens = token_set(&with_tags, 3);
    let overlap = word_overlap_pre(query_tokens, &candidate_tokens);

    let fb_dampening = if feedback_score < 0 { 0.5 } else { 1.0 };
    let coverage_boost = 1.0 + (query_coverage_boost(overlap, scoring_params) - 1.0) * fb_dampening;
    let overlap_factor = 1.0 + overlap * scoring_params.word_overlap_weight * fb_dampening;
    let jaccard = jaccard_pre(query_tokens, &candidate_tokens);
    let jaccard_factor = 1.0 + jaccard * scoring_params.jaccard_weight;
    let fb_factor = feedback_factor(feedback_score, scoring_params);
    let td = time_decay_et(created_at, et, scoring_params);
    let importance_factor_val =
        scoring_params.importance_floor + importance * scoring_params.importance_scale;

    let factor =
        coverage_boost * overlap_factor * jaccard_factor * fb_factor * td * importance_factor_val;

    RefinementResult {
        factor,
        text_overlap: overlap,
        coverage_boost,
        importance_factor: importance_factor_val,
        feedback_factor: fb_factor,
        time_decay: td,
    }
}

/// Compute the score for a graph neighbor candidate (Phase 5).
///
/// Takes the seed score, edge weight, relation type, and candidate
/// properties to produce a final neighbor score.
#[allow(clippy::too_many_arguments)]
pub(super) fn score_graph_neighbor(
    seed_score: f64,
    edge_weight: f64,
    rel_type: &str,
    query_tokens: &HashSet<String>,
    content: &str,
    tags: &[String],
    importance: f64,
    feedback_score: i64,
    created_at: &str,
    et: &EventType,
    scoring_params: &ScoringParams,
) -> (f64, f64) {
    let mut neighbor_score = scoring_params.graph_neighbor_factor * seed_score * edge_weight;

    // Relation-type scoring: different edge types get different boosts.
    match rel_type {
        "PRECEDED_BY" => {
            neighbor_score *= scoring_params.preceded_by_boost;
        }
        "RELATES_TO" | "SIMILAR_TO" | "SHARES_THEME" | "PARALLEL_CONTEXT" => {
            neighbor_score *= scoring_params.entity_relation_boost;
        }
        _ => {}
    }

    let with_tags = if tags.is_empty() {
        content.to_string()
    } else {
        format!("{} {}", content, tags.join(" "))
    };
    let overlap = word_overlap_pre(query_tokens, &token_set(&with_tags, 3));
    let fb_dampening = if feedback_score < 0 { 0.5 } else { 1.0 };
    neighbor_score *= 1.0 + overlap * scoring_params.neighbor_word_overlap_weight * fb_dampening;
    neighbor_score *= time_decay_et(created_at, et, scoring_params);
    neighbor_score *= scoring_params.neighbor_importance_floor
        + importance * scoring_params.neighbor_importance_scale;
    neighbor_score *= feedback_factor(feedback_score, scoring_params);

    (neighbor_score, overlap)
}

/// Compute the score for an entity-expansion candidate (Phase 5b).
///
/// Returns `(final_score, overlap, importance_factor, time_decay)` so the
/// caller can populate explain metadata.
#[allow(clippy::too_many_arguments)]
pub(super) fn score_entity_candidate(
    query_tokens: &HashSet<String>,
    content: &str,
    tags: &[String],
    importance: f64,
    created_at: &str,
    et: &EventType,
    priority_value: u8,
    scoring_params: &ScoringParams,
    max_seed_score: f64,
) -> (f64, f64, f64, f64) {
    let base_score = type_weight_et(et) * priority_factor(priority_value, scoring_params);
    let importance_factor_val =
        scoring_params.importance_floor + importance * scoring_params.importance_scale;

    let with_tags_text = if tags.is_empty() {
        content.to_string()
    } else {
        format!("{} {}", content, tags.join(" "))
    };
    let overlap = word_overlap_pre(query_tokens, &token_set(&with_tags_text, 3));
    let td = time_decay_et(created_at, et, scoring_params);

    let expanded_score = base_score
        * importance_factor_val
        * (1.0 + overlap * scoring_params.word_overlap_weight)
        * td
        * ENTITY_EXPANSION_BOOST;

    // Cap at a fraction of the max seed score
    let final_score = expanded_score.min(max_seed_score * 0.8);

    (final_score, overlap, importance_factor_val, td)
}

/// Return the entity expansion boost constant (for explain metadata).
#[inline]
pub(super) fn entity_expansion_boost() -> f64 {
    ENTITY_EXPANSION_BOOST
}
