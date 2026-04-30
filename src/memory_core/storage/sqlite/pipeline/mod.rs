//! Search pipeline sub-modules for the SQLite storage backend.
//!
//! The `advanced_search` flow is split into discrete phases that each live
//! in their own file:
//!
//! - [`retrieval`] — Phase 1 (vector candidates) and Phase 2 (FTS candidates)
//! - [`rerank`]    — Phase 2.5 cross-encoder reranking (real-embeddings only)
//! - [`fusion`]    — Phase 3 RRF fusion + dual-match boost + cross-encoder blend
//! - [`scoring`]   — Phase 4 score refinement and keyword-only result conversion
//! - [`enrichment`] — Phase 5 graph neighbor injection + Phase 5b entity-tag expansion
//! - [`abstention`] — Phase 6 dedup + abstention gate, plus hot-cache merge helpers
//! - [`decomp`]    — single-query pipeline runner for query decomposition
//!
//! All public-within-the-module items are reachable through this `mod.rs` so
//! callers (the residual `advanced.rs`) only need `use super::pipeline::*` or
//! `use super::pipeline::<phase>::<fn>`.

pub(super) mod abstention;
pub(super) mod decomp;
pub(super) mod enrichment;
pub(super) mod fusion;
pub(super) mod rerank;
pub(super) mod retrieval;
pub(super) mod scoring;

pub(super) const ADVANCED_FTS_CANDIDATE_MULTIPLIER: usize = 20;
pub(super) const ADVANCED_FTS_CANDIDATE_MIN: usize = 100;
pub(super) const ADVANCED_FTS_CANDIDATE_MAX: usize = 5_000;

pub(super) fn advanced_fts_candidate_limit(limit: usize) -> usize {
    let oversampled_limit = limit
        .saturating_mul(ADVANCED_FTS_CANDIDATE_MULTIPLIER)
        .clamp(ADVANCED_FTS_CANDIDATE_MIN, ADVANCED_FTS_CANDIDATE_MAX);
    oversampled_limit.max(limit)
}

// ── Re-exports for the residual `advanced.rs` module ────────────────────
//
// Phase functions are declared `pub(super)` inside their respective
// sub-modules (visible only within `pipeline/`). Re-exporting them here as
// `pub(super) use` makes them visible to the parent `sqlite/` module so
// `advanced.rs` can call them as `pipeline::collect_vector_candidates`, etc.

pub(super) use abstention::{
    abstain_and_dedup, merge_hot_cache_results, merge_semantic_metadata, merge_semantic_result,
};
pub(super) use decomp::run_single_query_pipeline;
pub(super) use enrichment::{enrich_graph_neighbors, expand_entity_tags};
pub(super) use fusion::fuse_and_score;
pub(super) use rerank::compute_cross_encoder_scores;
pub(super) use retrieval::{collect_fts_candidates, collect_vector_candidates};
pub(super) use scoring::{keyword_candidates_to_results, refine_scores};
