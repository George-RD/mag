use crate::memory_core::ScoringParams;
use crate::memory_core::storage::sqlite::RankedSemanticCandidate;

/// Extension point for scoring pipeline customization.
///
/// `ScoringStrategy` abstracts the score-refinement step of the search
/// pipeline. `DefaultScoringStrategy` wraps the existing multi-factor
/// scoring logic; future implementations (e.g., `KeywordOnlyStrategy`)
/// can replace it without modifying storage.
pub trait ScoringStrategy: Send + Sync {
    fn score(
        &self,
        candidate: &RankedSemanticCandidate,
        query: &str,
        params: &ScoringParams,
    ) -> f64;
}

/// The default scoring strategy that delegates to the candidate's existing score.
///
/// This is a thin wrapper that preserves the pre-computed score from the
/// multi-factor pipeline. The actual delegation into `fuse_refine_and_output`
/// is wired in PR-2d.
pub struct DefaultScoringStrategy;

impl DefaultScoringStrategy {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultScoringStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoringStrategy for DefaultScoringStrategy {
    fn score(
        &self,
        candidate: &RankedSemanticCandidate,
        _query: &str,
        _params: &ScoringParams,
    ) -> f64 {
        candidate.score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::SemanticResult;

    fn make_candidate(score: f64) -> RankedSemanticCandidate {
        #[allow(clippy::cast_possible_truncation)]
        let score_f32 = score as f32;
        RankedSemanticCandidate {
            result: SemanticResult {
                id: "test-id".to_string(),
                content: "test content".to_string(),
                tags: vec![],
                importance: 0.5,
                metadata: serde_json::json!({}),
                event_type: None,
                session_id: None,
                project: None,
                entity_id: None,
                agent_type: None,
                score: score_f32,
            },
            created_at: "2024-01-01T00:00:00Z".to_string(),
            event_at: "2024-01-01T00:00:00Z".to_string(),
            score,
            priority_value: 1,
            vec_sim: None,
            text_overlap: 0.0,
            entity_id: None,
            agent_type: None,
            explain: None,
        }
    }

    #[test]
    fn default_strategy_returns_candidate_score() {
        let strategy = DefaultScoringStrategy::new();
        let candidate = make_candidate(0.75);
        let params = ScoringParams::default();
        let result = strategy.score(&candidate, "query text", &params);
        assert!((result - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn default_strategy_ignores_query_and_params() {
        let strategy = DefaultScoringStrategy;
        let candidate = make_candidate(0.42);
        let params = ScoringParams::default();
        // Different query strings should not affect the score.
        assert!((strategy.score(&candidate, "foo", &params) - 0.42).abs() < f64::EPSILON);
        assert!((strategy.score(&candidate, "bar", &params) - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn scoring_strategy_is_object_safe() {
        // Verify the trait can be used as a dynamic dispatch object.
        let strategy: Box<dyn ScoringStrategy> = Box::new(DefaultScoringStrategy::new());
        let candidate = make_candidate(1.0);
        let params = ScoringParams::default();
        assert!((strategy.score(&candidate, "q", &params) - 1.0).abs() < f64::EPSILON);
    }
}
