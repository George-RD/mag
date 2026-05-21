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

pub(crate) mod abstention;
pub(crate) mod decomp;
pub(crate) mod enrichment;
pub(crate) mod fusion;
pub(crate) mod rerank;
pub(crate) mod retrieval;
pub(crate) mod scoring;

pub(crate) const ADVANCED_FTS_CANDIDATE_MULTIPLIER: usize = 20;
pub(crate) const ADVANCED_FTS_CANDIDATE_MIN: usize = 100;
pub(crate) const ADVANCED_FTS_CANDIDATE_MAX: usize = 5_000;

pub(crate) fn advanced_fts_candidate_limit(limit: usize) -> usize {
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
// `advanced.rs` can call them as e.g. `pipeline::collect_dual_candidates`.

pub(crate) use abstention::merge_hot_cache_results;
pub(crate) use decomp::enrich_with_decomposition;
pub(crate) use fusion::fuse_and_score;
pub(crate) use rerank::compute_cross_encoder_scores;
pub(crate) use retrieval::{collect_dual_candidates, collect_fts_candidates};
pub(crate) use scoring::run_keyword_only_search;
