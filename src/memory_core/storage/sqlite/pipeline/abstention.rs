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
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for candidate in ranked.into_values() {
        if !matches_search_options(&candidate, opts) {
            continue;
        }
        let fingerprint = normalize_for_dedup(&candidate.result.content);
        if seen.insert(fingerprint) {
            deduped.push(candidate);
        }
    }

    // Apply abstention gate on the filtered (in-scope) candidates.
    if !query_tokens.is_empty() {
        let max_text_overlap = deduped
            .iter()
            .map(|c| c.text_overlap)
            .fold(0.0f64, f64::max);
        if max_text_overlap < scoring_params.abstention_min_text {
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

    deduped.sort_by(|a, b| b.score.total_cmp(&a.score));
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
        if let Some(existing) = merged.get_mut(&hot_result.id) {
            merge_semantic_result(existing, hot_result);
        }
    }

    let mut merged_results: Vec<_> = merged.into_values().collect();
    merged_results.sort_by(|left, right| {
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
    deduped.sort_by(|left, right| {
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
