use crate::memory_core::{ScoringParams, SearchOptions, SemanticResult};
use crate::substrate::traits::{
    FusionStrategy, LifecyclePolicy, MultiFactorScorer, RetrievalStrategy, RrfFusion, Scorer,
    TtlExpirationPolicy,
};
use crate::substrate::types::{CandidateSet, QueryContext, ScoredCandidate};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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
fn rrf_fusion_pure_rank_ignores_raw_score() {
    // Regression: the current implementation incorrectly multiplies the
    // candidate's raw per-strategy score into the RRF term.  The documented
    // contract is `rrf_score(rank) = weight / (k + rank + 1)`; raw scores
    // should influence the result only through their rank position.
    let fusion = RrfFusion;
    let mut sets = HashMap::new();
    // Vector order: low raw score is ranked first, huge raw score is second.
    // A pure rank-based fusion must still prefer the first-ranked candidate.
    sets.insert(
        "vector",
        vec![
            (
                "low_raw_first".to_string(),
                0.01,
                make_candidate("low_raw_first", 0.01),
            ),
            (
                "high_raw_second".to_string(),
                100.0,
                make_candidate("high_raw_second", 100.0),
            ),
            ("dual".to_string(), 0.5, make_candidate("dual", 0.5)),
        ],
    );
    // FTS order: the dual-match candidate is ranked first with a huge raw score.
    // Its dual-match boost should not hide the fact that RRF itself is rank-only.
    sets.insert(
        "fts",
        vec![("dual".to_string(), 999.0, make_candidate("dual", 999.0))],
    );

    let result = fusion.fuse(sets, &ScoringParams::default());
    assert_eq!(result.len(), 3);
    let by_id: HashMap<String, f64> = result
        .iter()
        .map(|c| (c.result.id.clone(), c.score))
        .collect();

    let k = ScoringParams::default().rrf_k;
    let vec_rank_0 = 1.0 / (k + 0.0 + 1.0); // low_raw_first
    let vec_rank_1 = 1.0 / (k + 1.0 + 1.0); // high_raw_second
    let vec_rank_2 = 1.0 / (k + 2.0 + 1.0); // dual (vector arm)
    let fts_rank_0 = 1.0 / (k + 0.0 + 1.0); // dual (fts arm)
    let dual_boost = 1.5 + (1.0 / (1.0 + 0.0)) * 0.5;

    let expected_low_raw_first = vec_rank_0;
    let expected_high_raw_second = vec_rank_1;
    let expected_dual = (vec_rank_2 + fts_rank_0) * dual_boost;

    let eps = 1e-12;
    assert!(
        (by_id["low_raw_first"] - expected_low_raw_first).abs() < eps,
        "low_raw_first score {} != expected {}",
        by_id["low_raw_first"],
        expected_low_raw_first
    );
    assert!(
        (by_id["high_raw_second"] - expected_high_raw_second).abs() < eps,
        "high_raw_second score {} != expected {}",
        by_id["high_raw_second"],
        expected_high_raw_second
    );
    assert!(
        (by_id["dual"] - expected_dual).abs() < eps,
        "dual score {} != expected {}",
        by_id["dual"],
        expected_dual
    );

    // Rank-based ordering: dual match wins, and the first-ranked vector-only
    // candidate beats the second-ranked vector-only candidate even though its
    // raw score is much smaller.
    let ids: Vec<&str> = result.iter().map(|c| c.result.id.as_str()).collect();
    assert_eq!(ids, vec!["dual", "low_raw_first", "high_raw_second"]);
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

#[test]
fn ttl_policy_dead_when_expires_at_invalid_string() {
    let policy = TtlExpirationPolicy;
    let mut candidate = make_candidate("x", 1.0);
    candidate.result.metadata = serde_json::json!({"expires_at": "not-an-rfc3339-timestamp"});
    assert!(!policy.is_alive(&candidate));
}

#[test]
fn ttl_policy_dead_when_expires_at_non_string() {
    let policy = TtlExpirationPolicy;
    let mut candidate = make_candidate("x", 1.0);
    candidate.result.metadata = serde_json::json!({"expires_at": 12345});
    assert!(!policy.is_alive(&candidate));
}

#[test]
fn ttl_policy_alive_with_metadata_but_no_expires_at() {
    let policy = TtlExpirationPolicy;
    let mut candidate = make_candidate("x", 1.0);
    candidate.result.metadata = serde_json::json!({"retention": "permanent"});
    assert!(policy.is_alive(&candidate));
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

#[test]
fn cross_encoder_rejects_mismatched_ids_and_scores() {
    // Regression: CrossEncoderScorer::score_batch zips requested IDs with the
    // cross-encoder score vector without checking they have the same length.
    // A truncated or extra score must be rejected rather than silently ignored.
    use crate::substrate::enrichment_impl::apply_cross_encoder_scores;

    let mut candidates = HashMap::new();
    candidates.insert("a".to_string(), make_candidate("a", 0.5));
    candidates.insert("b".to_string(), make_candidate("b", 0.5));

    let original_scores: HashMap<String, f64> = candidates
        .iter()
        .map(|(id, c)| (id.clone(), c.score))
        .collect();

    let ids = vec!["a".to_string(), "b".to_string()];
    let ce_scores = vec![0.8f32]; // fewer scores than IDs

    let result = apply_cross_encoder_scores(&mut candidates, &ids, &ce_scores, 0.5);
    assert!(
        result.is_err(),
        "expected mismatched id/score lengths to be rejected"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("2"),
        "expected error message to mention id count, got: {msg}"
    );
    assert!(
        msg.contains("1"),
        "expected error message to mention score count, got: {msg}"
    );

    for (id, original) in original_scores {
        assert_eq!(
            candidates.get(&id).unwrap().score,
            original,
            "candidate {id} must not be mutated when id/score lengths mismatch"
        );
    }
}
#[tokio::test]
async fn search_pipeline_populates_query_tokens_for_scorer() {
    // Regression: SearchPipeline must lazily populate QueryContext.query_tokens
    // before invoking scorers. Without this, scorers that rely on the token set
    // have to re-derive it or silently operate without word-overlap signals.
    use crate::memory_core::scoring::token_set;
    use crate::substrate::SearchPipeline;
    use async_trait::async_trait;

    struct FakeRetrieval;

    #[async_trait]
    impl RetrievalStrategy for FakeRetrieval {
        fn name(&self) -> &str {
            "fake"
        }

        async fn collect(&self, _ctx: &QueryContext) -> anyhow::Result<CandidateSet> {
            Ok(vec![("a".to_string(), 1.0, make_candidate("a", 1.0))])
        }
    }

    struct FakeFusion;

    impl FusionStrategy for FakeFusion {
        fn fuse(
            &self,
            candidates: HashMap<&str, CandidateSet>,
            _scoring_params: &ScoringParams,
        ) -> Vec<ScoredCandidate> {
            candidates
                .into_values()
                .flat_map(|set| set.into_iter().map(|(_, _, candidate)| candidate))
                .collect()
        }
    }

    struct TokenSpy {
        tokens: Arc<Mutex<Option<HashSet<String>>>>,
    }

    #[async_trait]
    impl Scorer for TokenSpy {
        fn name(&self) -> &str {
            "token_spy"
        }

        async fn score_batch(
            &self,
            candidates: &mut HashMap<String, ScoredCandidate>,
            ctx: &QueryContext,
        ) -> anyhow::Result<()> {
            *self.tokens.lock().unwrap() = ctx.query_tokens.clone();
            // Drive text_overlap above the abstention gate so the assertion below
            // is about the scorer's input, not an empty result from abstention.
            for candidate in candidates.values_mut() {
                candidate.text_overlap = 1.0;
            }
            Ok(())
        }
    }

    let observed: Arc<Mutex<Option<HashSet<String>>>> = Arc::new(Mutex::new(None));
    let pipeline = SearchPipeline {
        retrieval: vec![Box::new(FakeRetrieval)],
        fusion: Box::new(FakeFusion),
        scorers: vec![Box::new(TokenSpy {
            tokens: observed.clone(),
        })],
        lifecycle: None,
        abstention_min_text: 0.15,
    };

    let query = "rust memory search";
    let ctx = QueryContext {
        query: query.to_string(),
        limit: 10,
        opts: SearchOptions::default(),
        scoring_params: ScoringParams::default(),
        query_embedding: None,
        query_tokens: None,
        include_superseded: false,
    };

    let results = pipeline.search(ctx).await.expect("search should succeed");
    assert!(
        !results.is_empty(),
        "expected results after scorer ran; abstention gate may have hidden the scorer input"
    );

    let observed_tokens = observed.lock().unwrap().take();
    assert!(
        observed_tokens.is_some(),
        "scorer observed QueryContext.query_tokens = None, but SearchPipeline must populate it"
    );
    let observed_tokens = observed_tokens.unwrap();
    let expected_tokens = token_set(query, 3);
    assert_eq!(
        observed_tokens, expected_tokens,
        "SearchPipeline must populate QueryContext.query_tokens before calling scorers"
    );
}

#[tokio::test]
async fn search_pipeline_explain_mode_uses_documented_metadata_key() {
    use crate::substrate::SearchPipeline;
    use async_trait::async_trait;

    struct FakeRetrieval;

    #[async_trait]
    impl RetrievalStrategy for FakeRetrieval {
        fn name(&self) -> &str {
            "fake"
        }

        async fn collect(&self, _ctx: &QueryContext) -> anyhow::Result<CandidateSet> {
            let mut candidate = make_candidate("a", 1.0);
            candidate.explain = Some(serde_json::json!({
                "strategy": "fake",
                "raw_score": 1.0,
            }));
            Ok(vec![("a".to_string(), 1.0, candidate)])
        }
    }

    struct FakeFusion;

    impl FusionStrategy for FakeFusion {
        fn fuse(
            &self,
            candidates: HashMap<&str, CandidateSet>,
            _scoring_params: &ScoringParams,
        ) -> Vec<ScoredCandidate> {
            candidates
                .into_values()
                .flat_map(|set| set.into_iter().map(|(_, _, candidate)| candidate))
                .collect()
        }
    }

    struct PassThroughScorer;

    #[async_trait]
    impl Scorer for PassThroughScorer {
        fn name(&self) -> &str {
            "pass_through"
        }

        async fn score_batch(
            &self,
            candidates: &mut HashMap<String, ScoredCandidate>,
            _ctx: &QueryContext,
        ) -> anyhow::Result<()> {
            for candidate in candidates.values_mut() {
                candidate.text_overlap = 1.0;
            }
            Ok(())
        }
    }

    let pipeline = SearchPipeline {
        retrieval: vec![Box::new(FakeRetrieval)],
        fusion: Box::new(FakeFusion),
        scorers: vec![Box::new(PassThroughScorer)],
        lifecycle: None,
        abstention_min_text: 0.15,
    };

    let results = pipeline
        .search(QueryContext {
            query: "rust memory search".to_string(),
            limit: 1,
            opts: SearchOptions {
                explain: Some(true),
                ..SearchOptions::default()
            },
            scoring_params: ScoringParams::default(),
            query_embedding: None,
            query_tokens: None,
            include_superseded: false,
        })
        .await
        .expect("search should succeed");

    assert_eq!(results.len(), 1, "expected a single explain-mode result");

    let metadata = results[0]
        .metadata
        .as_object()
        .expect("search results should carry object metadata");
    let explain = metadata
        .get("_explain")
        .expect("explain-mode results must expose explanation metadata under _explain");
    assert_eq!(explain["strategy"], serde_json::json!("fake"));
    assert_eq!(explain["raw_score"], serde_json::json!(1.0));
    assert!(
        !metadata.contains_key("explain"),
        "legacy explain metadata key must not be present"
    );
}
