use mag::memory_core::embedder::PlaceholderEmbedder;
use mag::memory_core::{MemoryInput, ScoringParams, SearchOptions, storage::SqliteStorage};
use mag::substrate::orchestrators::SearchPipeline;
use mag::substrate::types::QueryContext;
use mag::substrate::{
    FullTextSearch, MemoryStore, MultiFactorScorer, RrfFusion, TtlExpirationPolicy,
};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
async fn substrate_fts_pipeline_returns_results() {
    let storage =
        SqliteStorage::new_with_path(PathBuf::from(":memory:"), Arc::new(PlaceholderEmbedder))
            .unwrap();
    let store: Arc<dyn MemoryStore> = Arc::new(storage);

    store
        .store(
            "mem-1",
            "rust programming language",
            &MemoryInput {
                content: "rust programming language".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    store
        .store(
            "mem-2",
            "python programming language",
            &MemoryInput {
                content: "python programming language".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let pipeline = SearchPipeline {
        retrieval: vec![Box::new(FullTextSearch {
            store: Arc::clone(&store),
        })],
        fusion: Box::new(RrfFusion),
        scorers: vec![Box::new(MultiFactorScorer)],
        lifecycle: Some(Box::new(TtlExpirationPolicy)),
        abstention_min_text: 0.15,
    };

    let ctx = QueryContext {
        query: "programming language".to_string(),
        limit: 10,
        opts: SearchOptions::default(),
        scoring_params: ScoringParams::default(),
        query_embedding: None,
        query_tokens: None,
        include_superseded: false,
    };

    let results = pipeline.search(ctx).await.unwrap();
    assert!(!results.is_empty(), "pipeline should return results");
}
