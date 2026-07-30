use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{
    AdvancedSearcher, MemoryInput, Pipeline, PlaceholderEmbedder, PlaceholderPipeline,
    SearchOptions,
};

fn legacy_pipeline(storage: &SqliteStorage) -> Pipeline {
    Pipeline::new(
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
    )
}

#[tokio::test]
async fn local_runtime_preserves_store_retrieve_search_and_advanced_search_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new_with_path(
        temp.path().join("memory.db"),
        Arc::new(PlaceholderEmbedder),
    )
    .unwrap();
    let legacy = legacy_pipeline(&storage);
    let runtime = LocalMemoryRuntime::from_storage(storage.clone());

    let content = "portable sqlite context survives tool switches";
    let input = MemoryInput {
        id: Some("runtime-parity-1".to_string()),
        content: content.to_string(),
        tags: vec!["runtime".to_string(), "parity".to_string()],
        metadata: serde_json::json!({"source": "runtime-parity"}),
        ..Default::default()
    };

    let id = runtime.store(content, &input).await.unwrap();
    assert_eq!(id, "runtime-parity-1");

    let runtime_retrieved = runtime.retrieve(&id).await.unwrap();
    let legacy_retrieved = legacy.retrieve(&id).await.unwrap();
    assert_eq!(runtime_retrieved, legacy_retrieved);
    assert_eq!(runtime_retrieved, format!("processed: {content}"));

    let options = SearchOptions::default();
    let runtime_search = runtime.search("portable sqlite", 10, &options).await.unwrap();
    let legacy_search = legacy.search("portable sqlite", 10, &options).await.unwrap();
    assert_eq!(runtime_search, legacy_search);
    assert_eq!(runtime_search.len(), 1);
    assert_eq!(runtime_search[0].id, id);

    let runtime_advanced = runtime
        .advanced_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    let direct_advanced = storage
        .advanced_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    assert_eq!(runtime_advanced, direct_advanced);
    assert_eq!(runtime_advanced.len(), 1);
    assert_eq!(runtime_advanced[0].id, id);
}
