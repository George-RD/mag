use std::path::PathBuf;
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{
    AdvancedSearcher, Deleter, EventType, GraphTraverser, Lister, MemoryInput, MemoryUpdate,
    PhraseSearcher, Pipeline, PlaceholderEmbedder, PlaceholderPipeline, RelationshipQuerier,
    SearchOptions, SimilarFinder, VersionChainQuerier,
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

async fn storage_at(path: PathBuf) -> SqliteStorage {
    tokio::task::spawn_blocking(move || {
        SqliteStorage::new_with_path(path, Arc::new(PlaceholderEmbedder))
    })
    .await
    .expect("SQLite initialization task should not panic")
    .expect("SQLite storage should initialize")
}

#[tokio::test]
async fn local_runtime_preserves_supported_capability_outputs() {
    let runtime_temp = tempfile::tempdir().unwrap();
    let legacy_temp = tempfile::tempdir().unwrap();
    let runtime_storage = storage_at(runtime_temp.path().join("memory.db")).await;
    let legacy_storage = storage_at(legacy_temp.path().join("memory.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(runtime_storage.clone());
    let legacy = legacy_pipeline(&legacy_storage);

    let content = "portable sqlite context survives tool switches";
    let input = MemoryInput {
        id: Some("runtime-parity-1".to_string()),
        content: content.to_string(),
        tags: vec!["runtime".to_string(), "parity".to_string()],
        metadata: serde_json::json!({"source": "runtime-parity"}),
        ..Default::default()
    };

    let runtime_id = runtime.store(content, &input).await.unwrap();
    let legacy_id = legacy.run(content, &input).await.unwrap();
    assert_eq!(runtime_id, legacy_id);

    let runtime_retrieved = runtime.retrieve(&runtime_id).await.unwrap();
    let legacy_retrieved = legacy.retrieve(&legacy_id).await.unwrap();
    assert_eq!(runtime_retrieved, legacy_retrieved);
    assert_eq!(runtime_retrieved, format!("processed: {content}"));

    let options = SearchOptions::default();
    let runtime_search = runtime
        .search("portable sqlite", 10, &options)
        .await
        .unwrap();
    let legacy_search = legacy
        .search("portable sqlite", 10, &options)
        .await
        .unwrap();
    assert_eq!(runtime_search, legacy_search);
    assert_eq!(runtime_search.len(), 1);
    assert_eq!(runtime_search[0].id, runtime_id);

    let runtime_recent = runtime.recent(10, &options).await.unwrap();
    let legacy_recent = legacy.recent(10, &options).await.unwrap();
    assert_eq!(runtime_recent, legacy_recent);
    assert_eq!(runtime_recent.len(), 1);
    assert_eq!(runtime_recent[0].id, runtime_id);

    let runtime_phrase = runtime
        .phrase_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    let direct_phrase = legacy_storage
        .phrase_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    assert_eq!(runtime_phrase, direct_phrase);
    assert_eq!(runtime_phrase.len(), 1);
    assert_eq!(runtime_phrase[0].id, runtime_id);

    let runtime_semantic = runtime
        .semantic_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    let legacy_semantic = legacy
        .semantic_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    assert_eq!(runtime_semantic, legacy_semantic);
    assert_eq!(runtime_semantic.len(), 1);
    assert_eq!(runtime_semantic[0].id, runtime_id);

    let runtime_advanced = runtime
        .advanced_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    let direct_advanced = runtime_storage
        .advanced_search("portable sqlite", 10, &options)
        .await
        .unwrap();
    assert_eq!(runtime_advanced, direct_advanced);
    assert_eq!(runtime_advanced.len(), 1);
    assert_eq!(runtime_advanced[0].id, runtime_id);

    let updated_content = "portable sqlite context updated through runtime";
    let update = MemoryUpdate {
        content: Some(updated_content.to_string()),
        tags: Some(vec!["runtime".to_string(), "updated".to_string()]),
        importance: Some(0.9),
        metadata: Some(serde_json::json!({"source": "runtime-update-parity"})),
        event_type: Some(EventType::LessonLearned),
        priority: Some(9),
    };
    runtime.update(&runtime_id, &update).await.unwrap();
    legacy_storage.update(&legacy_id, &update).await.unwrap();

    let runtime_updated = runtime.retrieve(&runtime_id).await.unwrap();
    let legacy_updated = legacy.retrieve(&legacy_id).await.unwrap();
    assert_eq!(runtime_updated, legacy_updated);
    assert_eq!(runtime_updated, updated_content);

    let runtime_list = runtime.list(0, 10, &options).await.unwrap();
    let direct_list = legacy_storage.list(0, 10, &options).await.unwrap();
    assert_eq!(runtime_list, direct_list);
    assert_eq!(runtime_list.total, 1);
    assert_eq!(runtime_list.memories[0].id, runtime_id);

    let runtime_relations = runtime.get_relationships(&runtime_id).await.unwrap();
    let direct_relations = legacy_storage.get_relationships(&legacy_id).await.unwrap();
    assert_eq!(runtime_relations, direct_relations);
    assert!(runtime_relations.is_empty());

    let runtime_traversal = runtime.traverse(&runtime_id, 2, 0.0, None).await.unwrap();
    let direct_traversal = legacy_storage
        .traverse(&legacy_id, 2, 0.0, None)
        .await
        .unwrap();
    assert_eq!(runtime_traversal, direct_traversal);
    assert!(runtime_traversal.is_empty());

    let runtime_chain = runtime.version_chain(&runtime_id).await.unwrap();
    let direct_chain = legacy_storage.get_version_chain(&legacy_id).await.unwrap();
    assert_eq!(runtime_chain, direct_chain);
    assert_eq!(runtime_chain.len(), 1);
    assert_eq!(runtime_chain[0].id, runtime_id);

    let related_content = "offline indexing remains available to future local tools";
    let related_input = MemoryInput {
        id: Some("runtime-parity-2".to_string()),
        content: related_content.to_string(),
        tags: vec!["runtime".to_string(), "similar".to_string()],
        metadata: serde_json::json!({"source": "runtime-similar-parity"}),
        ..Default::default()
    };
    let runtime_related_id = runtime
        .store(related_content, &related_input)
        .await
        .unwrap();
    let legacy_related_id = legacy.run(related_content, &related_input).await.unwrap();
    assert_eq!(runtime_related_id, legacy_related_id);

    let runtime_similar = runtime.find_similar(&runtime_id, 1).await.unwrap();
    let direct_similar = legacy_storage.find_similar(&legacy_id, 1).await.unwrap();
    assert_eq!(runtime_similar, direct_similar);
    assert_eq!(runtime_similar.len(), 1);
    assert_eq!(runtime_similar[0].id, runtime_related_id);

    let runtime_deleted = runtime.delete(&runtime_id).await.unwrap();
    let legacy_deleted = legacy_storage.delete(&legacy_id).await.unwrap();
    assert_eq!(runtime_deleted, legacy_deleted);
    assert!(runtime_deleted);
    assert!(runtime.retrieve(&runtime_id).await.is_err());
    assert!(legacy.retrieve(&legacy_id).await.is_err());
}
