//! Phase 6: Collection-level abstention + dedup, and hot-cache merge helpers.
//!
//! `abstain_and_dedup` is the tail of the original `fuse_refine_and_output`
//! function — it applies the abstention gate, dedupes the surviving
//! candidates, runs the pluggable [`ScoringStrategy`] as a final pass, and
//! shapes the result list (with optional explain metadata).
//!
//! `merge_hot_cache_results`, `merge_semantic_result`, and
//! `merge_semantic_metadata` blend hot-tier cache hits into the final
//! collection-level result list.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::super::helpers::{matches_search_options, normalize_for_dedup};
use super::super::storage::RankedSemanticCandidate;
use crate::memory_core::scoring_strategy::ScoringStrategy;
use crate::memory_core::{ScoringParams, SearchOptions, SemanticResult};

// Dense embeddings have a high similarity floor, so vector similarity alone is
// unsafe as an abstention signal. A unique strong match with clear separation
// from the runner-up can rescue genuine paraphrases that have no lexical overlap
// without returning an arbitrary nearest neighbour.
const SEMANTIC_RESCUE_MIN_SIM: f64 = 0.82;
const SEMANTIC_RESCUE_MIN_MARGIN: f64 = 0.035;

fn has_confident_semantic_match(candidates: &[RankedSemanticCandidate]) -> bool {
    let mut top = f64::NEG_INFINITY;
    let mut runner_up = f64::NEG_INFINITY;
    let mut strong_matches = 0usize;

    for similarity in candidates.iter().filter_map(|candidate| candidate.vec_sim) {
        if !similarity.is_finite() {
            continue;
        }
        if similarity >= SEMANTIC_RESCUE_MIN_SIM {
            strong_matches += 1;
        }
        if similarity > top {
            runner_up = top;
            top = similarity;
        } else if similarity > runner_up {
            runner_up = similarity;
        }
    }

    strong_matches == 1
        && top >= SEMANTIC_RESCUE_MIN_SIM
        && (!runner_up.is_finite() || top - runner_up >= SEMANTIC_RESCUE_MIN_MARGIN)
}

/// Phase 6: collection-level abstention + dedup, plus the final scoring-strategy
/// pass and result-list shaping (normalization, explain injection).
#[allow(clippy::too_many_arguments)]
pub(crate) fn abstain_and_dedup(
    ranked: HashMap<String, RankedSemanticCandidate>,
    query_tokens: &HashSet<String>,
    opts: &SearchOptions,
    limit: usize,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
    scoring_strategy: &dyn ScoringStrategy,
    query: &str,
) -> Result<Vec<SemanticResult>> {
    // ── Phase 6: Collection-level abstention + dedup ─────────────
    // Dedup by content fingerprint, keeping the highest-scoring candidate per
    // fingerprint. HashMap iteration order is nondeterministic, so a
    // first-wins policy would surface arbitrary duplicates.
    let mut by_fingerprint: HashMap<String, RankedSemanticCandidate> = HashMap::new();
    for candidate in ranked.into_values() {
        if !matches_search_options(&candidate, opts) {
            continue;
        }
        let fingerprint = normalize_for_dedup(&candidate.result.content);
        match by_fingerprint.entry(fingerprint) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if candidate.score > entry.get().score {
                    *entry.get_mut() = candidate;
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
        }
    }
    let mut deduped: Vec<RankedSemanticCandidate> = by_fingerprint.into_values().collect();

    // Lexical evidence remains the primary precision guard. When wording differs
    // completely, allow a semantic rescue only for one strong, unambiguous vector
    // match. One retrieval channel should not veto another confident channel, but
    // several merely-near candidates still indicate that MAG should abstain.
    if !query_tokens.is_empty() {
        let max_text_overlap = deduped
            .iter()
            .map(|c| c.text_overlap)
            .fold(0.0f64, f64::max);
        let lexical_match = max_text_overlap >= scoring_params.abstention_min_text;
        if !lexical_match && !has_confident_semantic_match(&deduped) {
            return Ok(Vec::new());
        }
    }
    if deduped.is_empty() {
        return Ok(Vec::new());
    }

    // Apply the pluggable scoring strategy as a final pass.
    for candidate in &mut deduped {
        candidate.score = scoring_strategy.score(candidate, query, scoring_params);
    }

    deduped.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
    let max_score = deduped.first().map(|c| c.score).unwrap_or(0.0);
    let mut out = Vec::new();
    for mut candidate in deduped.into_iter().take(limit) {
        let normalized = if max_score > 0.0 {
            (candidate.score / max_score).clamp(0.0, 1.0)
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let normalized_f32 = normalized as f32;
        candidate.result.score = normalized_f32;

        // Always inject text_overlap for confidence computation
        if let serde_json::Value::Object(ref mut meta) = candidate.result.metadata {
            meta.insert(
                "_text_overlap".to_string(),
                serde_json::json!(candidate.text_overlap),
            );
        }

        // Inject explain data into result metadata when enabled
        if explain_enabled && let Some(mut exp) = candidate.explain.take() {
            exp["final_score"] = serde_json::json!(normalized);
            if let serde_json::Value::Object(ref mut meta) = candidate.result.metadata {
                meta.insert("_explain".to_string(), exp);
            }
        }

        out.push(candidate.result);
    }
    Ok(out)
}

/// Merge hot-tier cache hits into a freshly computed result list.
pub(crate) fn merge_hot_cache_results(
    hot_results: Vec<SemanticResult>,
    mut results: Vec<SemanticResult>,
    limit: usize,
) -> Vec<SemanticResult> {
    if hot_results.is_empty() || limit == 0 {
        results.truncate(limit);
        return results;
    }

    let mut merged: HashMap<String, SemanticResult> = results
        .drain(..)
        .map(|result| (result.id.clone(), result))
        .collect();

    for hot_result in hot_results {
        match merged.entry(hot_result.id.clone()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                merge_semantic_result(entry.get_mut(), hot_result);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(hot_result);
            }
        }
    }

    let mut merged_results: Vec<_> = merged.into_values().collect();
    merged_results.sort_unstable_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut deduped = Vec::new();
    let mut seen = HashMap::new();
    for result in merged_results {
        let fingerprint = normalize_for_dedup(&result.content);
        if let Some(existing_idx) = seen.get(&fingerprint).copied() {
            merge_semantic_result(&mut deduped[existing_idx], result);
            continue;
        }
        seen.insert(fingerprint, deduped.len());
        deduped.push(result);
    }
    deduped.sort_unstable_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    deduped.truncate(limit);
    deduped
}

pub(crate) fn merge_semantic_result(existing: &mut SemanticResult, incoming: SemanticResult) {
    if incoming.score > existing.score
        || (incoming.score == existing.score && incoming.id < existing.id)
    {
        let mut replacement = incoming;
        merge_semantic_metadata(
            &mut replacement.metadata,
            std::mem::take(&mut existing.metadata),
        );
        replacement.score = replacement.score.max(existing.score);
        *existing = replacement;
        return;
    }

    existing.score = existing.score.max(incoming.score);
    merge_semantic_metadata(&mut existing.metadata, incoming.metadata);
}

pub(crate) fn merge_semantic_metadata(
    existing: &mut serde_json::Value,
    incoming: serde_json::Value,
) {
    if let (serde_json::Value::Object(existing_meta), serde_json::Value::Object(incoming_meta)) =
        (existing, incoming)
    {
        for (key, value) in incoming_meta {
            existing_meta.entry(key).or_insert(value);
        }
    }
}
