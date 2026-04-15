use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::memory_core::storage::sqlite::RankedSemanticCandidate;
use crate::memory_core::{AdvancedSearcher, ScoringParams, SearchOptions, SemanticResult};

// ── Supporting types ────────────────────────────────────────────────────

/// An ordered set of candidates produced by a `RetrievalStrategy`.
///
/// Each entry is `(memory_id, raw_score, candidate)` where `raw_score` is
/// signal-native (cosine similarity for vector, raw BM25 for FTS).
///
/// Aligned with `trait-surface.md` §3.2.
pub type CandidateSet = Vec<(String, f64, RankedSemanticCandidate)>;

/// Read-path context passed through the retrieval pipeline.
///
/// Wraps the query string, limit, search options, scoring params, and
/// pre-computed embedding needed by retrieval strategies.
///
/// Aligned with `trait-surface.md` §2.3.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Consumed by RetrievalStrategy impls in v0.3.x pipeline composition
pub struct QueryContext {
    /// Raw query string from the caller.
    pub query: String,
    /// Maximum number of results to return after the full pipeline.
    pub limit: usize,
    /// Filter and feature options (event_type, project, session, explain, etc.).
    pub opts: SearchOptions,
    /// Scoring knobs. Consumers should clone from a shared `Arc<ScoringParams>`.
    pub scoring_params: ScoringParams,
    /// Pre-computed query embedding. `None` until the embedding stage
    /// populates it; strategies that do not need embeddings ignore it.
    #[allow(dead_code)] // Consumed by vector strategy in v0.3.x pipeline composition
    pub query_embedding: Option<Vec<f32>>,
    /// Whether superseded memories should be included in candidate sets.
    pub include_superseded: bool,
}

// ── Trait ────────────────────────────────────────────────────────────────

/// Retrieves an unscored, unranked candidate set for a single retrieval signal.
///
/// Implementors MUST NOT apply multi-signal fusion -- that is `FusionStrategy`'s
/// job (PR-4a-ii+). The returned `CandidateSet` is `(memory_id, raw_score,
/// candidate)` where `raw_score` is signal-native (cosine similarity for vector;
/// raw BM25 for FTS, where more-negative = better).
///
/// Aligned with `trait-surface.md` §3.2.
#[async_trait]
#[allow(dead_code)] // Consumed by downstream pipeline composition (v0.3.x)
pub trait RetrievalStrategy: Send + Sync {
    /// Human-readable name used for logging and as the key in fusion dispatch.
    fn name(&self) -> &str;

    /// Collect candidates for the given query context.
    ///
    /// Implementors that perform blocking I/O (e.g. ONNX inference, SQLite reads)
    /// MUST wrap the blocking work in `tokio::task::spawn_blocking`.
    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet>;
}

// ── FullPipelineStrategy ────────────────────────────────────────────────

/// Reference implementation that wraps the existing 6-phase search pipeline.
///
/// Delegates to `AdvancedSearcher::advanced_search` on the underlying
/// `SqliteStorage`, then converts the `Vec<SemanticResult>` output into a
/// `CandidateSet`. This is a thin adapter -- no pipeline logic is duplicated.
///
/// Construction requires an `Arc<dyn AdvancedSearcher>` which in practice
/// is the `SqliteStorage` instance.
#[allow(dead_code)] // Consumed by downstream pipeline composition (v0.3.x)
pub struct FullPipelineStrategy {
    searcher: Arc<dyn AdvancedSearcher>,
}

#[allow(dead_code)] // Consumed by downstream pipeline composition (v0.3.x)
impl FullPipelineStrategy {
    /// Create a new `FullPipelineStrategy` wrapping the given searcher.
    pub fn new(searcher: Arc<dyn AdvancedSearcher>) -> Self {
        Self { searcher }
    }
}

#[async_trait]
impl RetrievalStrategy for FullPipelineStrategy {
    fn name(&self) -> &str {
        "full-pipeline"
    }

    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet> {
        let results: Vec<SemanticResult> = self
            .searcher
            .advanced_search(&ctx.query, ctx.limit, &ctx.opts)
            .await?;

        // Convert SemanticResult -> CandidateSet entries.
        // The full pipeline already scores candidates internally, so we
        // propagate the score as the raw_score signal and build a
        // RankedSemanticCandidate shell around each result.
        //
        // NOTE: SemanticResult does not carry created_at, event_at,
        // priority_value, or text_overlap. These fields are set to
        // defaults here. Downstream consumers of FullPipelineStrategy
        // output MUST NOT rely on these fields for fusion or ranking --
        // the score field already incorporates them from the inner
        // pipeline. PR-4a-ii will wire dispatch such that these
        // placeholder values are never inspected.
        let candidates: CandidateSet = results
            .into_iter()
            .map(|result| {
                let id = result.id.clone();
                let score = f64::from(result.score);
                let candidate = RankedSemanticCandidate {
                    created_at: String::new(),
                    event_at: String::new(),
                    score,
                    priority_value: 1,
                    vec_sim: None,
                    text_overlap: 0.0,
                    entity_id: result.entity_id.clone(),
                    agent_type: result.agent_type.clone(),
                    explain: None,
                    result,
                };
                (id, score, candidate)
            })
            .collect();

        Ok(candidates)
    }
}

// ── FtsSearcher trait ───────────────────────────────────────────────────

/// Pure FTS5 BM25 search — no embeddings, no reranker.
///
/// Returns raw FTS candidates as a `CandidateSet` where `raw_score` is
/// the BM25 rank value (more-negative = better match). Implementors wrap
/// the existing `collect_fts_candidates` infrastructure.
///
/// This trait exists to decouple `KeywordOnlyStrategy` from direct
/// `SqliteStorage` internals, following the same `Arc<dyn Trait>` pattern
/// used by `FullPipelineStrategy` with `AdvancedSearcher`.
#[async_trait]
pub trait FtsSearcher: Send + Sync {
    /// Collect FTS5 BM25 candidates for the given query.
    ///
    /// `limit` is the maximum number of candidates to return.
    /// `opts` controls filters (event_type, project, session, etc.).
    /// `include_superseded` controls whether superseded memories appear.
    /// `scoring_params` provides scoring knobs for initial score computation.
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &ScoringParams,
    ) -> Result<CandidateSet>;
}

// ── KeywordOnlyStrategy ────────────────────────────────────────────────

/// FTS5-only retrieval strategy for keyword-intent queries.
///
/// When the query classifier identifies a query as `QueryIntent::Keyword`
/// (code identifiers, file paths, snake_case/CamelCase tokens), this
/// strategy skips embedding computation and vector search entirely,
/// returning only FTS5 BM25 candidates.
///
/// This reduces latency (~8ms embedding savings) while maintaining
/// retrieval quality for keyword lookups where exact term matching
/// outperforms semantic similarity.
#[allow(dead_code)] // Strategy wired via FtsSearcher dispatch; trait-based dispatch in v0.3.x
pub struct KeywordOnlyStrategy {
    fts_searcher: Arc<dyn FtsSearcher>,
}

#[allow(dead_code)] // Strategy wired via FtsSearcher dispatch; trait-based dispatch in v0.3.x
impl KeywordOnlyStrategy {
    /// Create a new `KeywordOnlyStrategy` wrapping the given FTS searcher.
    pub fn new(fts_searcher: Arc<dyn FtsSearcher>) -> Self {
        Self { fts_searcher }
    }
}

#[async_trait]
impl RetrievalStrategy for KeywordOnlyStrategy {
    fn name(&self) -> &str {
        "keyword-only"
    }

    async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet> {
        self.fts_searcher
            .fts_search(
                &ctx.query,
                ctx.limit,
                &ctx.opts,
                ctx.include_superseded,
                &ctx.scoring_params,
            )
            .await
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::SemanticResult;

    /// Stub searcher that returns a fixed result set.
    struct StubSearcher {
        results: Vec<SemanticResult>,
    }

    #[async_trait]
    impl AdvancedSearcher for StubSearcher {
        async fn advanced_search(
            &self,
            _query: &str,
            _limit: usize,
            _opts: &SearchOptions,
        ) -> Result<Vec<SemanticResult>> {
            Ok(self.results.clone())
        }
    }

    fn make_semantic_result(id: &str, score: f32) -> SemanticResult {
        SemanticResult {
            id: id.to_string(),
            content: format!("content for {id}"),
            tags: vec![],
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
            entity_id: None,
            agent_type: None,
            score,
        }
    }

    fn make_query_context(query: &str) -> QueryContext {
        QueryContext {
            query: query.to_string(),
            limit: 10,
            opts: SearchOptions::default(),
            scoring_params: ScoringParams::default(),
            query_embedding: None,
            include_superseded: false,
        }
    }

    #[test]
    fn full_pipeline_strategy_name() {
        let searcher = Arc::new(StubSearcher { results: vec![] });
        let strategy = FullPipelineStrategy::new(searcher);
        assert_eq!(strategy.name(), "full-pipeline");
    }

    #[test]
    fn full_pipeline_strategy_construction() {
        let searcher = Arc::new(StubSearcher {
            results: vec![make_semantic_result("a", 0.9)],
        });
        let strategy = FullPipelineStrategy::new(searcher);
        // Verify the strategy implements the trait via dynamic dispatch.
        let _boxed: Box<dyn RetrievalStrategy> = Box::new(strategy);
    }

    #[test]
    fn retrieval_strategy_is_object_safe() {
        // Verify the trait can be used as Arc<dyn RetrievalStrategy>.
        let searcher = Arc::new(StubSearcher { results: vec![] });
        let strategy = FullPipelineStrategy::new(searcher);
        let _arc: Arc<dyn RetrievalStrategy> = Arc::new(strategy);
    }

    #[tokio::test]
    async fn full_pipeline_collect_delegates_to_searcher() {
        let searcher = Arc::new(StubSearcher {
            results: vec![
                make_semantic_result("mem-1", 0.85),
                make_semantic_result("mem-2", 0.72),
            ],
        });
        let strategy = FullPipelineStrategy::new(searcher);
        let ctx = make_query_context("test query");

        let candidates = strategy.collect(&ctx).await.unwrap();

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].0, "mem-1");
        assert!((candidates[0].1 - 0.85_f64).abs() < 0.01);
        assert_eq!(candidates[1].0, "mem-2");
        assert!((candidates[1].1 - 0.72_f64).abs() < 0.01);
    }

    #[tokio::test]
    async fn full_pipeline_collect_empty_results() {
        let searcher = Arc::new(StubSearcher { results: vec![] });
        let strategy = FullPipelineStrategy::new(searcher);
        let ctx = make_query_context("nothing here");

        let candidates = strategy.collect(&ctx).await.unwrap();
        assert!(candidates.is_empty());
    }

    // ── KeywordOnlyStrategy tests ──────────────────────────────────────

    /// Stub FTS searcher that returns a fixed candidate set.
    struct StubFtsSearcher {
        candidates: CandidateSet,
    }

    #[async_trait]
    impl FtsSearcher for StubFtsSearcher {
        async fn fts_search(
            &self,
            _query: &str,
            _limit: usize,
            _opts: &SearchOptions,
            _include_superseded: bool,
            _scoring_params: &ScoringParams,
        ) -> Result<CandidateSet> {
            Ok(self.candidates.clone())
        }
    }

    fn make_fts_candidate(id: &str, bm25_score: f64) -> (String, f64, RankedSemanticCandidate) {
        (
            id.to_string(),
            bm25_score,
            RankedSemanticCandidate {
                created_at: String::new(),
                event_at: String::new(),
                score: 1.0,
                priority_value: 1,
                vec_sim: None,
                text_overlap: 0.0,
                entity_id: None,
                agent_type: None,
                explain: None,
                result: SemanticResult {
                    id: id.to_string(),
                    content: format!("fts content for {id}"),
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
            },
        )
    }

    #[test]
    fn keyword_only_strategy_name() {
        let fts = Arc::new(StubFtsSearcher { candidates: vec![] });
        let strategy = KeywordOnlyStrategy::new(fts);
        assert_eq!(strategy.name(), "keyword-only");
    }

    #[test]
    fn keyword_only_strategy_construction() {
        let fts = Arc::new(StubFtsSearcher {
            candidates: vec![make_fts_candidate("k1", -5.0)],
        });
        let strategy = KeywordOnlyStrategy::new(fts);
        // Verify the strategy implements the trait via dynamic dispatch.
        let _boxed: Box<dyn RetrievalStrategy> = Box::new(strategy);
    }

    #[test]
    fn keyword_only_strategy_is_object_safe() {
        let fts = Arc::new(StubFtsSearcher { candidates: vec![] });
        let strategy = KeywordOnlyStrategy::new(fts);
        let _arc: Arc<dyn RetrievalStrategy> = Arc::new(strategy);
    }

    #[tokio::test]
    async fn keyword_only_collect_delegates_to_fts_searcher() {
        let fts = Arc::new(StubFtsSearcher {
            candidates: vec![
                make_fts_candidate("kw-1", -8.5),
                make_fts_candidate("kw-2", -3.2),
            ],
        });
        let strategy = KeywordOnlyStrategy::new(fts);
        let ctx = make_query_context("SqliteStorage");

        let candidates = strategy.collect(&ctx).await.unwrap();

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].0, "kw-1");
        assert!((candidates[0].1 - (-8.5_f64)).abs() < 0.01);
        assert_eq!(candidates[1].0, "kw-2");
        assert!((candidates[1].1 - (-3.2_f64)).abs() < 0.01);
    }

    #[tokio::test]
    async fn keyword_only_collect_empty_results() {
        let fts = Arc::new(StubFtsSearcher { candidates: vec![] });
        let strategy = KeywordOnlyStrategy::new(fts);
        let ctx = make_query_context("nonexistent_function");

        let candidates = strategy.collect(&ctx).await.unwrap();
        assert!(candidates.is_empty());
    }

    #[tokio::test]
    async fn keyword_only_passes_context_fields() {
        // Verify that QueryContext fields are correctly threaded through.
        use std::sync::atomic::{AtomicBool, Ordering};

        struct CapturingFtsSearcher {
            called: AtomicBool,
        }

        #[async_trait]
        impl FtsSearcher for CapturingFtsSearcher {
            async fn fts_search(
                &self,
                query: &str,
                limit: usize,
                _opts: &SearchOptions,
                include_superseded: bool,
                _scoring_params: &ScoringParams,
            ) -> Result<CandidateSet> {
                self.called.store(true, Ordering::SeqCst);
                assert_eq!(query, "my_func");
                assert_eq!(limit, 5);
                assert!(!include_superseded);
                Ok(vec![])
            }
        }

        let fts = Arc::new(CapturingFtsSearcher {
            called: AtomicBool::new(false),
        });
        let strategy = KeywordOnlyStrategy::new(fts.clone());
        let ctx = QueryContext {
            query: "my_func".to_string(),
            limit: 5,
            opts: SearchOptions::default(),
            scoring_params: ScoringParams::default(),
            query_embedding: None,
            include_superseded: false,
        };

        let _ = strategy.collect(&ctx).await.unwrap();
        assert!(fts.called.load(Ordering::SeqCst));
    }
}
