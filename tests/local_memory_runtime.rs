use std::path::PathBuf;
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, EventType, GraphTraverser,
    Lister, MemoryInput, MemoryUpdate, PhraseSearcher, Pipeline, PlaceholderEmbedder,
    PlaceholderPipeline, RelationshipQuerier, ReminderManager, SearchOptions, SimilarFinder,
    VersionChainQuerier,
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

    let relationship_metadata = serde_json::json!({"source": "runtime-traversal-parity"});
    runtime_storage
        .add_relationship(
            &runtime_id,
            &runtime_related_id,
            "TRAVERSAL_PARITY",
            1.0,
            &relationship_metadata,
        )
        .await
        .unwrap();
    legacy_storage
        .add_relationship(
            &legacy_id,
            &legacy_related_id,
            "TRAVERSAL_PARITY",
            1.0,
            &relationship_metadata,
        )
        .await
        .unwrap();

    let runtime_traversal = runtime.traverse(&runtime_id, 2, 0.0, None).await.unwrap();
    let direct_traversal = legacy_storage
        .traverse(&legacy_id, 2, 0.0, None)
        .await
        .unwrap();
    assert_eq!(runtime_traversal.len(), 1);
    assert_eq!(runtime_traversal.len(), direct_traversal.len());
    for (runtime_node, direct_node) in runtime_traversal.iter().zip(&direct_traversal) {
        assert_eq!(runtime_node.id, direct_node.id);
        assert_eq!(runtime_node.content, direct_node.content);
        assert_eq!(runtime_node.event_type, direct_node.event_type);
        assert_eq!(runtime_node.metadata, direct_node.metadata);
        assert_eq!(runtime_node.hop, direct_node.hop);
        assert_eq!(runtime_node.weight, direct_node.weight);
        assert_eq!(runtime_node.edge_type, direct_node.edge_type);
    }
    assert_eq!(runtime_traversal[0].id, runtime_related_id);
    assert_eq!(runtime_traversal[0].hop, 1);
    assert_eq!(runtime_traversal[0].weight, 1.0);
    assert_eq!(runtime_traversal[0].edge_type, "TRAVERSAL_PARITY");

    let runtime_similar = runtime.find_similar(&runtime_id, 1).await.unwrap();
    let direct_similar = legacy_storage.find_similar(&legacy_id, 1).await.unwrap();
    assert_eq!(runtime_similar, direct_similar);
    assert_eq!(runtime_similar.len(), 1);
    assert_eq!(runtime_similar[0].id, runtime_related_id);

    let checkpoint_input = CheckpointInput {
        task_title: "runtime checkpoint parity".to_string(),
        progress: "session state is preserved".to_string(),
        plan: Some("keep continuity local".to_string()),
        files_touched: Some(serde_json::json!(["src/local_memory_runtime.rs"])),
        decisions: Some(vec!["delegate without semantic drift".to_string()]),
        key_context: Some("checkpoint parity test".to_string()),
        next_steps: Some("migrate reminders next".to_string()),
        session_id: Some("runtime-checkpoint-session".to_string()),
        project: Some("runtime-checkpoint-project".to_string()),
    };
    let runtime_checkpoint_id = runtime
        .save_checkpoint(checkpoint_input.clone())
        .await
        .unwrap();
    let direct_checkpoint_id = legacy_storage
        .save_checkpoint(checkpoint_input)
        .await
        .unwrap();
    uuid::Uuid::parse_str(&runtime_checkpoint_id).unwrap();
    uuid::Uuid::parse_str(&direct_checkpoint_id).unwrap();

    let runtime_checkpoints = runtime
        .resume_task(
            "runtime checkpoint parity",
            Some("runtime-checkpoint-project"),
            10,
        )
        .await
        .unwrap();
    let direct_checkpoints = legacy_storage
        .resume_task(
            "runtime checkpoint parity",
            Some("runtime-checkpoint-project"),
            10,
        )
        .await
        .unwrap();
    assert_eq!(runtime_checkpoints.len(), 1);
    assert_eq!(direct_checkpoints.len(), 1);
    assert_eq!(
        runtime_checkpoints[0]["content"],
        direct_checkpoints[0]["content"]
    );
    assert_eq!(
        runtime_checkpoints[0]["metadata"],
        direct_checkpoints[0]["metadata"]
    );
    assert_eq!(runtime_checkpoints[0]["metadata"]["checkpoint_number"], 1);
    chrono::DateTime::parse_from_rfc3339(runtime_checkpoints[0]["created_at"].as_str().unwrap())
        .unwrap();
    assert!(
        runtime
            .resume_task("runtime checkpoint parity", Some("other-project"), 10,)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        runtime
            .resume_task("missing checkpoint", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        runtime
            .resume_task("runtime checkpoint parity", None, 0)
            .await
            .unwrap()
            .is_empty()
    );

    let runtime_deleted = runtime.delete(&runtime_id).await.unwrap();
    let legacy_deleted = legacy_storage.delete(&legacy_id).await.unwrap();
    assert_eq!(runtime_deleted, legacy_deleted);
    assert!(runtime_deleted);
    assert!(runtime.retrieve(&runtime_id).await.is_err());
    assert!(legacy.retrieve(&legacy_id).await.is_err());
}

fn stable_reminder_contract(entry: &serde_json::Value) -> serde_json::Value {
    let text = entry["text"]
        .as_str()
        .and_then(|value| value.split_once("\n[due: ").map(|(text, _)| text))
        .unwrap_or_default();
    serde_json::json!({
        "text": text,
        "status": entry["status"],
        "is_due": entry["is_due"],
        "is_overdue": entry["is_overdue"],
        "metadata": {
            "event_type": entry["metadata"]["event_type"],
            "reminder_status": entry["metadata"]["reminder_status"],
            "context": entry["metadata"]["context"],
            "session_id": entry["metadata"]["session_id"],
            "project": entry["metadata"]["project"],
        }
    })
}

#[tokio::test]
async fn local_runtime_preserves_reminder_capability_outputs() {
    let runtime_temp = tempfile::tempdir().unwrap();
    let direct_temp = tempfile::tempdir().unwrap();
    let runtime_storage = storage_at(runtime_temp.path().join("memory.db")).await;
    let direct_storage = storage_at(direct_temp.path().join("memory.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(runtime_storage);

    let runtime_created = runtime
        .create_reminder(
            "runtime reminder parity",
            "2h",
            Some("after checkpoint migration"),
            Some("runtime-reminder-session"),
            Some("runtime-reminder-project"),
        )
        .await
        .unwrap();
    let direct_created = direct_storage
        .create_reminder(
            "runtime reminder parity",
            "2h",
            Some("after checkpoint migration"),
            Some("runtime-reminder-session"),
            Some("runtime-reminder-project"),
        )
        .await
        .unwrap();

    for created in [&runtime_created, &direct_created] {
        uuid::Uuid::parse_str(created["reminder_id"].as_str().unwrap()).unwrap();
        chrono::DateTime::parse_from_rfc3339(created["remind_at"].as_str().unwrap()).unwrap();
        assert_eq!(created["text"], "runtime reminder parity");
        assert_eq!(created["duration"], "2h");
    }

    let runtime_id = runtime_created["reminder_id"].as_str().unwrap();
    let direct_id = direct_created["reminder_id"].as_str().unwrap();
    let runtime_pending = runtime.list_reminders(Some("pending")).await.unwrap();
    let direct_pending = direct_storage
        .list_reminders(Some("pending"))
        .await
        .unwrap();
    assert_eq!(runtime_pending.len(), 1);
    assert_eq!(direct_pending.len(), 1);
    assert_eq!(runtime_pending[0]["reminder_id"], runtime_id);
    assert_eq!(direct_pending[0]["reminder_id"], direct_id);
    assert_eq!(
        stable_reminder_contract(&runtime_pending[0]),
        stable_reminder_contract(&direct_pending[0])
    );
    for entry in [&runtime_pending[0], &direct_pending[0]] {
        chrono::DateTime::parse_from_rfc3339(entry["remind_at"].as_str().unwrap()).unwrap();
        chrono::DateTime::parse_from_rfc3339(entry["created_at"].as_str().unwrap()).unwrap();
        chrono::DateTime::parse_from_rfc3339(entry["metadata"]["created_at_utc"].as_str().unwrap())
            .unwrap();
    }

    let runtime_dismissed = runtime.dismiss_reminder(runtime_id).await.unwrap();
    let direct_dismissed = direct_storage.dismiss_reminder(direct_id).await.unwrap();
    assert_eq!(runtime_dismissed["status"], direct_dismissed["status"]);
    assert_eq!(runtime_dismissed["status"], "dismissed");
    for dismissed in [&runtime_dismissed, &direct_dismissed] {
        chrono::DateTime::parse_from_rfc3339(dismissed["dismissed_at"].as_str().unwrap()).unwrap();
    }

    assert!(
        runtime
            .list_reminders(Some("pending"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        direct_storage
            .list_reminders(Some("pending"))
            .await
            .unwrap()
            .is_empty()
    );
    let runtime_dismissed_list = runtime.list_reminders(Some("dismissed")).await.unwrap();
    let direct_dismissed_list = direct_storage
        .list_reminders(Some("dismissed"))
        .await
        .unwrap();
    assert_eq!(runtime_dismissed_list.len(), 1);
    assert_eq!(direct_dismissed_list.len(), 1);
    assert_eq!(
        stable_reminder_contract(&runtime_dismissed_list[0]),
        stable_reminder_contract(&direct_dismissed_list[0])
    );
    assert_eq!(runtime.list_reminders(Some("all")).await.unwrap().len(), 1);
    assert_eq!(
        direct_storage
            .list_reminders(Some("all"))
            .await
            .unwrap()
            .len(),
        1
    );
}
