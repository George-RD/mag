use crate::memory_core::{ScoringParams, SearchOptions, SemanticResult};
use crate::substrate::traits::{
    FusionStrategy, LifecyclePolicy, MultiFactorScorer, RrfFusion, Scorer, TtlExpirationPolicy,
};
use crate::substrate::types::{QueryContext, ScoredCandidate};
use std::collections::HashMap;

fn make_candidate(id: &str, score: f64) -> ScoredCandidate {
    ScoredCandidate {
        result: SemanticResult {
            id: id.to_string(),
            content: format!("content {}", id),
            tags: vec![],
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
            entity_id: None,
            agent_type: None,
            score: 0.0,
        },
        created_at: String::new(),
        event_at: String::new(),
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
fn rrf_fusion_ranks_candidates() {
    let fusion = RrfFusion;
    let mut sets = HashMap::new();
    sets.insert(
        "vector",
        vec![
            ("a".to_string(), 1.0, make_candidate("a", 1.0)),
            ("b".to_string(), 0.9, make_candidate("b", 0.9)),
        ],
    );
    sets.insert(
        "fts",
        vec![
            ("b".to_string(), 1.0, make_candidate("b", 1.0)),
            ("c".to_string(), 0.8, make_candidate("c", 0.8)),
        ],
    );
    let result = fusion.fuse(sets, &ScoringParams::default());
    assert!(!result.is_empty());
}

#[test]
fn ttl_policy_alive_by_default() {
    let policy = TtlExpirationPolicy;
    let candidate = make_candidate("x", 1.0);
    assert!(policy.is_alive(&candidate));
}

#[test]
fn ttl_policy_dead_when_expired() {
    let policy = TtlExpirationPolicy;
    let mut candidate = make_candidate("x", 1.0);
    candidate.result.metadata = serde_json::json!({"expires_at": "2000-01-01T00:00:00Z"});
    assert!(!policy.is_alive(&candidate));
}

#[tokio::test]
async fn multi_factor_scorer_does_not_panic() {
    let scorer = MultiFactorScorer;
    let mut candidates = HashMap::new();
    candidates.insert("a".to_string(), make_candidate("a", 1.0));
    let ctx = QueryContext {
        query: "test".to_string(),
        limit: 10,
        opts: SearchOptions::default(),
        scoring_params: ScoringParams::default(),
        query_embedding: None,
        query_tokens: None,
        include_superseded: false,
    };
    scorer.score_batch(&mut candidates, &ctx).await.unwrap();
    assert!(!candidates.is_empty());
}
