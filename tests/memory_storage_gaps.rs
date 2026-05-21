use mag::memory_core::embedder::PlaceholderEmbedder;
use mag::memory_core::scoring_strategy::DefaultScoringStrategy;
use mag::memory_core::storage::memory::MemoryStorage;
use mag::memory_core::{AdvancedSearcher, MemoryInput, PhraseSearcher, SearchOptions, Storage};
use std::sync::Arc;

#[tokio::test]
async fn memory_storage_advanced_search_finds_by_content() {
    let storage = MemoryStorage::new(
        Arc::new(PlaceholderEmbedder),
        Arc::new(DefaultScoringStrategy::new()),
    );

    storage
        .store(
            "m1",
            "hello world rust",
            &MemoryInput {
                content: "hello world rust".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    storage
        .store(
            "m2",
            "goodbye python",
            &MemoryInput {
                content: "goodbye python".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let results = storage
        .advanced_search("rust", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert!(
        results.iter().any(|r| r.id == "m1"),
        "should find m1 containing 'rust'"
    );
    assert_eq!(results[0].id, "m1");
}

#[tokio::test]
async fn memory_storage_phrase_search_finds_by_phrase() {
    let storage = MemoryStorage::new(
        Arc::new(PlaceholderEmbedder),
        Arc::new(DefaultScoringStrategy::new()),
    );

    storage
        .store(
            "m1",
            "hello world rust",
            &MemoryInput {
                content: "hello world rust".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let results = storage
        .phrase_search("world rust", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "m1");
}
