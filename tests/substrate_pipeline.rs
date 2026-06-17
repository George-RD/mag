use mag::memory_core::embedder::{Embedder, PlaceholderEmbedder};
use mag::memory_core::{MemoryInput, ScoringParams, SearchOptions, storage::SqliteStorage};
use mag::substrate::orchestrators::SearchPipeline;
use mag::substrate::types::{QueryContext, WriteContext};
use mag::substrate::{
    EmbedAndExtractPipeline, FullTextSearch, IngestionPipeline, MemoryStore, MultiFactorScorer,
    RrfFusion, TtlExpirationPolicy,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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

#[tokio::test]
async fn substrate_embed_and_extract_reuses_precomputed_embedding() {
    #[derive(Clone)]
    struct CountingEmbedder {
        counter: Arc<AtomicUsize>,
    }

    impl Embedder for CountingEmbedder {
        fn dimension(&self) -> usize {
            32
        }

        fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            Ok(vec![1.0_f32; self.dimension()])
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let embedder: Arc<dyn Embedder> = Arc::new(CountingEmbedder {
        counter: Arc::clone(&counter),
    });
    let storage =
        SqliteStorage::new_with_path(PathBuf::from(":memory:"), Arc::clone(&embedder)).unwrap();
    let store: Arc<dyn MemoryStore> = Arc::new(storage);

    let pipeline = EmbedAndExtractPipeline::new(Arc::clone(&embedder));

    let ctx = WriteContext {
        input: MemoryInput {
            content: "rust programming language".to_string(),
            ..Default::default()
        },
        assigned_id: "mem-1".to_string(),
        embedding: None,
    };

    let id = pipeline.ingest(ctx, store.as_ref()).await.unwrap();
    assert_eq!(id, "mem-1");

    let count = counter.load(Ordering::SeqCst);
    assert_eq!(
        count, 1,
        "embedder should be invoked exactly once for a single ingest with a precomputed embedding, got {count}"
    );
}
