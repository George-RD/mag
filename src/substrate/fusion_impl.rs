use std::collections::{HashMap, HashSet};

use crate::memory_core::scoring::ScoringParams;
use crate::substrate::traits::FusionStrategy;
use crate::substrate::types::{CandidateSet, ScoredCandidate};

impl FusionStrategy for super::traits::RrfFusion {
    fn fuse(
        &self,
        candidates: HashMap<&str, CandidateSet>,
        scoring_params: &ScoringParams,
    ) -> Vec<ScoredCandidate> {
        let total_candidates: usize = candidates.values().map(|v| v.len()).sum();
        let mut output: HashMap<String, ScoredCandidate> =
            HashMap::with_capacity(total_candidates);
        let mut in_vector: HashSet<String> = HashSet::new();
        let mut fts_ranks: HashMap<String, usize> = HashMap::new();
        for (strategy_name, candidate_set) in candidates {
            let weight = match strategy_name {
                "vector" => scoring_params.rrf_weight_vec,
                "fts" => scoring_params.rrf_weight_fts,
                _ => 1.0,
            };
            for (rank, (id, _raw_score, mut candidate)) in candidate_set.into_iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let rrf_score = weight / (scoring_params.rrf_k + rank as f64 + 1.0);
                if strategy_name == "vector" {
                    in_vector.insert(id.clone());
                } else if strategy_name == "fts" {
                    fts_ranks.insert(id.clone(), rank);
                }
                match output.entry(id) {
                    std::collections::hash_map::Entry::Occupied(mut e) => {
                        e.get_mut().score += candidate.score * rrf_score;
                    }
                    std::collections::hash_map::Entry::Vacant(e) => {
                        candidate.score *= rrf_score;
                        e.insert(candidate);
                    }
                }
            }
        }
        // Apply adaptive dual-match boost for candidates in both vector and FTS.
        for (id, candidate) in output.iter_mut() {
            if in_vector.contains(id)
                && let Some(&fts_rank) = fts_ranks.get(id)
            {
                #[allow(clippy::cast_precision_loss)]
                let text_rel = 1.0 / (1.0 + fts_rank as f64);
                let base = scoring_params.dual_match_boost.max(1.0);
                let adaptive_boost = base + text_rel * 0.5;
                candidate.score *= adaptive_boost;
            }
        }
        let mut results: Vec<ScoredCandidate> = output.into_values().collect();
        results.sort_unstable_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }
}
