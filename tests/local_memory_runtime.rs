use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{
    AdvancedSearcher, MemoryInput, Pipeline, PlaceholderEmbedder, PlaceholderPipeline,
    SearchOptions,
};

fn current_pipeline(storage: &SqliteStorage) -> Pipeline {
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

fn in_memory_storage() -> Result<SqliteStorage> {
    SqliteStorage::new_with_path(
        PathBuf::from(":memory:"),
        Arc::new(PlaceholderEmbedder),
    )
}

#[tokio::test]
async fn local_runtime_matches_current_sqlite_and_pipeline_surface() -> Result<()> {
    let direct_storage = in_memory_storage()?;
    let direct_pipeline = current_pipeline(&direct_storage);
    let runtime = LocalMemoryRuntime::from_storage(in_memory_storage()?);

    let input = MemoryInput {
        id: Some("local-runtime-parity".to_string()),
        content: "portable local runtime parity memory".to_string(),
        tags: vec!["runtime".to_string(), "local-first".to_string()],
        ..MemoryInput::default()
    };
    let options = SearchOptions::default();

    let direct_id = direct_pipeline.run("", &input).await?;
    let runtime_id = runtime.store(&input).await?;
    assert_eq!(runtime_id, direct_id);

    let direct_retrieved = direct_pipeline.retrieve(&direct_id).await?;
    let runtime_retrieved = runtime.retrieve(&runtime_id).await?;
    assert_eq!(runtime_retrieved, direct_retrieved);
    assert_eq!(runtime_retrieved, "processed: portable local runtime parity memory");

    let direct_search = direct_pipeline
        .search("portable local runtime parity", 10, &options)
        .await?;
    let runtime_search = runtime
        .search("portable local runtime parity", 10, &options)
        .await?;
    assert_eq!(runtime_search, direct_search);

    let direct_advanced = direct_storage
        .advanced_search("portable local runtime parity", 10, &options)
        .await?;
    let runtime_advanced = runtime
        .advanced_search("portable local runtime parity", 10, &options)
        .await?;
    assert_eq!(runtime_advanced, direct_advanced);
    assert!(runtime_advanced.iter().any(|result| result.id == runtime_id));

    Ok(())
}
