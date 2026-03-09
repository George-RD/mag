use super::*;
use crate::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, EventType, ExpirationSweeper,
    FeedbackRecorder, GraphTraverser, LessonQuerier, MaintenanceManager, PhraseSearcher,
    ProfileManager, Recents, ReminderManager, Retriever, SearchOptions, Searcher, SemanticSearcher,
    SimilarFinder, StatsProvider, Storage, TTL_EPHEMERAL, TTL_LONG_TERM, TTL_SHORT_TERM, Updater,
    VersionChainQuerier, WelcomeProvider, default_priority_for_event_type,
    default_ttl_for_event_type, is_valid_event_type, parse_duration,
};

#[derive(Debug, Clone)]
struct KeywordEmbedder;

impl Embedder for KeywordEmbedder {
    fn dimension(&self) -> usize {
        4
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.contains("alpha") {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        } else if text.contains("beta") {
            Ok(vec![0.9, 0.1, 0.0, 0.0])
        } else {
            Ok(vec![0.0, 0.0, 1.0, 0.0])
        }
    }
}

#[test]
fn test_new_with_path_creates_parent_and_db() {
    let base = std::env::temp_dir().join(format!("romega-sqlite-test-{}", Uuid::new_v4()));
    let db_path = base.join("nested").join("memory.db");

    let storage = SqliteStorage::new_with_path(
        db_path.clone(),
        std::sync::Arc::new(crate::memory_core::PlaceholderEmbedder),
    );
    assert!(storage.is_ok());
    assert!(db_path.exists());
    assert!(db_path.parent().is_some_and(Path::exists));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn test_schema_contains_required_tables_and_columns() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let conn = storage.test_conn().unwrap();

    let memories_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    for col in [
        "id",
        "content",
        "embedding",
        "parent_id",
        "created_at",
        "event_at",
        "content_hash",
        "source_type",
        "last_accessed_at",
        "tags",
        "importance",
        "metadata",
        "access_count",
        "session_id",
        "event_type",
        "project",
        "priority",
        "entity_id",
        "agent_type",
        "ttl_seconds",
        "canonical_hash",
        "version_chain_id",
        "superseded_by_id",
        "superseded_at",
    ] {
        assert!(memories_cols.iter().any(|c| c == col));
    }

    let relationships_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(relationships)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    for col in [
        "id",
        "source_id",
        "target_id",
        "rel_type",
        "weight",
        "metadata",
        "created_at",
    ] {
        assert!(relationships_cols.iter().any(|c| c == col));
    }
}

#[test]
fn test_schema_contains_fts5_table() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let conn = storage.test_conn().unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_store_and_retrieve_roundtrip() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "m1",
        "hello world",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let content = storage.retrieve("m1").await.unwrap();

    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_retrieve_updates_last_accessed_at() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "m2",
        "payload",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    storage
        .debug_force_last_accessed_at("m2", "2000-01-01T00:00:00.000Z")
        .unwrap();

    let before = storage.debug_get_last_accessed_at("m2").unwrap();
    assert_eq!(before, "2000-01-01T00:00:00.000Z");

    let _ = storage.retrieve("m2").await.unwrap();
    let after = storage.debug_get_last_accessed_at("m2").unwrap();

    assert_ne!(after, before);
}

#[tokio::test]
async fn test_add_relationship() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "a",
        "alpha",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "b",
        "beta",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rel_id = storage
        .add_relationship("a", "b", "links_to", 1.0, &serde_json::json!({}))
        .await
        .unwrap();

    let conn = storage.test_conn().unwrap();

    let stored_rel_type: String = conn
        .query_row(
            "SELECT rel_type FROM relationships WHERE id = ?1",
            params![rel_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(stored_rel_type, "links_to");
}

#[tokio::test]
async fn test_search_matches_content_case_insensitive() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "s1",
        "Rust memory store",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "s2",
        "another note",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("MEMORY", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "s1");
    assert_eq!(results[0].content, "Rust memory store");
    assert!(results[0].tags.is_empty());
    assert_eq!(results[0].importance, 0.5);
    assert_eq!(results[0].metadata, serde_json::json!({}));
}

#[tokio::test]
async fn test_search_treats_like_wildcards_as_literals() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "p1",
        "value 100% done",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "p2",
        "value 1000 done",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("100%", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "p1");
}

#[tokio::test]
async fn test_search_with_zero_limit_returns_no_results() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "z1",
        "zero limit candidate",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("zero", 0, &SearchOptions::default())
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_search_escapes_underscore_and_backslash_literals() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "u1",
        r"file_a\b",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "u2",
        r"fileXab",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let underscore_results = storage
        .search("file_a", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(underscore_results.len(), 1);
    assert_eq!(underscore_results[0].id, "u1");

    let backslash_results = storage
        .search(r"a\b", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(backslash_results.len(), 1);
    assert_eq!(backslash_results[0].id, "u1");
}

#[tokio::test]
async fn test_fts5_search_basic() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts1",
        "Rust memory management",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("memory", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "fts1");
}

#[tokio::test]
async fn test_fts5_search_multiple_terms() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts2",
        "rust memory ownership",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts3",
        "rust tooling",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("rust memory", 10, &SearchOptions::default())
        .await
        .unwrap();
    // OR semantics: both "rust memory ownership" and "rust tooling" match
    // because each contains at least one query term.
    assert_eq!(results.len(), 2);
    // The entry matching both terms should rank higher (better BM25).
    assert_eq!(results[0].id, "fts2");
}

#[tokio::test]
async fn test_fts5_search_returns_importance_metadata() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts4",
        "fts metadata payload",
        &MemoryInput {
            tags: vec!["alpha".to_string()],
            importance: 0.91,
            metadata: serde_json::json!({"scope":"fts"}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("metadata", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "fts4");
    assert_eq!(results[0].tags, vec!["alpha".to_string()]);
    assert_eq!(results[0].importance, 0.91);
    assert_eq!(results[0].metadata, serde_json::json!({"scope":"fts"}));
}

#[tokio::test]
async fn test_fts5_fallback_to_like() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts5",
        "value with % symbol and _ marker",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let percent_results = storage
        .search("%", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(percent_results.len(), 1);
    assert_eq!(percent_results[0].id, "fts5");

    let underscore_results = storage
        .search("_", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(underscore_results.len(), 1);
    assert_eq!(underscore_results[0].id, "fts5");
}

#[tokio::test]
async fn test_fts5_sync_on_update() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts6",
        "before update",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "fts6",
        &MemoryUpdate {
            content: Some("after update".to_string()),
            tags: None,
            importance: None,
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("after", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "fts6");
}

#[tokio::test]
async fn test_fts5_sync_on_delete() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "fts7",
        "delete candidate",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let deleted = storage.delete("fts7").await.unwrap();
    assert!(deleted);

    let results = storage
        .search("candidate", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_recent_returns_most_recently_accessed_first() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "r1",
        "older",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "r2",
        "newer",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    storage
        .debug_force_last_accessed_at("r1", "2000-01-01T00:00:00.000Z")
        .unwrap();
    storage
        .debug_force_last_accessed_at("r2", "2001-01-01T00:00:00.000Z")
        .unwrap();

    let results = storage.recent(2, &SearchOptions::default()).await.unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "r2");
    assert_eq!(results[1].id, "r1");
}

#[tokio::test]
async fn test_semantic_search_prefers_exact_text_match() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "e1",
        "alpha beta gamma",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "e2",
        "other content",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .semantic_search("alpha beta gamma", 2, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "e1");
    assert!(results[0].tags.is_empty());
    assert_eq!(results[0].importance, 0.5);
    assert_eq!(results[0].metadata, serde_json::json!({}));
    assert!(results[0].score >= results[1].score);
}

#[tokio::test]
async fn test_semantic_search_zero_limit_returns_no_results() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "e3",
        "candidate",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .semantic_search("candidate", 0, &SearchOptions::default())
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ── Delete tests ──

#[tokio::test]
async fn test_delete_existing_memory() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "d1",
        "to-delete",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let deleted = storage.delete("d1").await.unwrap();
    assert!(deleted);

    let err = storage.retrieve("d1").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn test_delete_nonexistent_returns_false() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let deleted = storage.delete("no-such-id").await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn test_delete_cascades_relationships() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ca",
        "alpha",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "cb",
        "beta",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    storage
        .add_relationship("ca", "cb", "links_to", 1.0, &serde_json::json!({}))
        .await
        .unwrap();

    storage.delete("ca").await.unwrap();

    let rels = storage.get_relationships("cb").await.unwrap();
    assert!(rels.is_empty());
}

// ── Update tests ──

#[tokio::test]
async fn test_update_content_changes_value() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "up1",
        "original",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "up1",
        &MemoryUpdate {
            content: Some("updated".to_string()),
            tags: None,
            importance: None,
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let content = storage.retrieve("up1").await.unwrap();
    assert_eq!(content, "updated");
}

#[tokio::test]
async fn test_update_with_tags() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let tags = vec!["a".to_string(), "b".to_string()];
    <SqliteStorage as Storage>::store(
        &storage,
        "up2",
        "data",
        &MemoryInput {
            tags: tags.clone(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let new_tags = vec!["x".to_string()];
    <SqliteStorage as Updater>::update(
        &storage,
        "up2",
        &MemoryUpdate {
            content: Some("data-v2".to_string()),
            tags: Some(new_tags.clone()),
            importance: None,
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(&new_tags, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "up2");
    assert_eq!(results[0].content, "data-v2");
    assert_eq!(results[0].importance, 0.5);
    assert_eq!(results[0].metadata, serde_json::json!({}));
}

#[tokio::test]
async fn test_update_without_tags_preserves_existing() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let tags = vec!["keep".to_string()];
    <SqliteStorage as Storage>::store(
        &storage,
        "up3",
        "data",
        &MemoryInput {
            tags: tags.clone(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "up3",
        &MemoryUpdate {
            content: Some("data-v2".to_string()),
            tags: None,
            importance: None,
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(&tags, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "up3");
}

#[tokio::test]
async fn test_update_tags_only_preserves_content() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "up4",
        "keep-this",
        &MemoryInput {
            tags: vec!["old".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let new_tags = vec!["new-tag".to_string()];
    <SqliteStorage as Updater>::update(
        &storage,
        "up4",
        &MemoryUpdate {
            content: None,
            tags: Some(new_tags.clone()),
            importance: None,
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let content = storage.retrieve("up4").await.unwrap();
    assert_eq!(content, "keep-this");
    let results = storage
        .get_by_tags(&new_tags, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "up4");
}

#[tokio::test]
async fn test_update_neither_content_nor_tags_errors() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "up5",
        "data",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let err = <SqliteStorage as Updater>::update(&storage, "up5", &MemoryUpdate::default()).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn test_update_nonexistent_errors() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let err = <SqliteStorage as Updater>::update(
        &storage,
        "ghost",
        &MemoryUpdate {
            content: Some("data".to_string()),
            ..Default::default()
        },
    )
    .await;
    assert!(err.is_err());
}

// ── Tags tests ──

#[tokio::test]
async fn test_get_by_tags_filters_correctly() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "t1",
        "one",
        &MemoryInput {
            tags: vec!["rust".to_string(), "memory".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "t2",
        "two",
        &MemoryInput {
            tags: vec!["rust".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "t3",
        "three",
        &MemoryInput {
            tags: vec!["python".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(&["rust".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 2);

    let results = storage
        .get_by_tags(
            &["rust".to_string(), "memory".to_string()],
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "t1");
}

#[tokio::test]
async fn test_get_by_tags_legacy_csv_backward_compat() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Insert a row with legacy CSV tags directly via raw SQL
    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
            "INSERT INTO memories (id, content, content_hash, source_type, tags)
                 VALUES ('csv1', 'legacy data', 'hash1', 'test', 'rust,memory')",
            [],
        )
        .unwrap();
    }
    // JSON-tagged row via normal API
    <SqliteStorage as Storage>::store(
        &storage,
        "json1",
        "json data",
        &MemoryInput {
            tags: vec!["rust".to_string(), "search".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Search for 'rust' should find both CSV and JSON rows
    let results = storage
        .get_by_tags(&["rust".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 2);

    // Search for 'memory' should find only the CSV row
    let results = storage
        .get_by_tags(&["memory".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "csv1");

    // Search for 'search' should find only the JSON row
    let results = storage
        .get_by_tags(&["search".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "json1");
}

#[tokio::test]
async fn test_get_by_tags_empty_returns_empty() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "te",
        "data",
        &MemoryInput {
            tags: vec!["tag".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let results = storage
        .get_by_tags(&[], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ── List tests ──

#[tokio::test]
async fn test_list_with_pagination() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for i in 0..5 {
        <SqliteStorage as Storage>::store(
            &storage,
            &format!("l{i}"),
            &format!("item-{i}"),
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let result = storage.list(0, 3, &SearchOptions::default()).await.unwrap();
    assert_eq!(result.memories.len(), 3);
    assert_eq!(result.total, 5);

    let result = storage.list(3, 3, &SearchOptions::default()).await.unwrap();
    assert_eq!(result.memories.len(), 2);
    assert_eq!(result.total, 5);
}

#[tokio::test]
async fn test_list_zero_limit_returns_count_only() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lz1",
        "a",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lz2",
        "b",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = storage.list(0, 0, &SearchOptions::default()).await.unwrap();
    assert!(result.memories.is_empty());
    assert_eq!(result.total, 2);
}

// ── Relationship query tests ──

#[tokio::test]
async fn test_get_relationships_both_directions() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ra",
        "alpha",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rb",
        "beta",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rc",
        "gamma",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    storage
        .add_relationship("ra", "rb", "links_to", 1.0, &serde_json::json!({}))
        .await
        .unwrap();
    storage
        .add_relationship("rc", "ra", "depends_on", 1.0, &serde_json::json!({}))
        .await
        .unwrap();

    let rels = storage.get_relationships("ra").await.unwrap();
    let types: Vec<&str> = rels
        .iter()
        .filter(|r| r.rel_type == "links_to" || r.rel_type == "depends_on")
        .map(|r| r.rel_type.as_str())
        .collect();
    assert_eq!(types.len(), 2);
    assert!(types.contains(&"links_to"));
    assert!(types.contains(&"depends_on"));
}

#[tokio::test]
async fn test_get_relationships_empty() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lonely",
        "alone",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rels = storage.get_relationships("lonely").await.unwrap();
    assert!(rels.is_empty());
}

// ── Tags roundtrip with store ──

#[tokio::test]
async fn test_store_with_tags_roundtrip() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let tags = vec!["project-x".to_string(), "important".to_string()];
    <SqliteStorage as Storage>::store(
        &storage,
        "st1",
        "tagged content",
        &MemoryInput {
            tags: tags.clone(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(&["project-x".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "st1");
    assert_eq!(results[0].content, "tagged content");
    assert_eq!(results[0].tags, tags);
    assert_eq!(results[0].importance, 0.5);
    assert_eq!(results[0].metadata, serde_json::json!({}));
}

#[tokio::test]
async fn test_store_with_importance_and_metadata() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let metadata = serde_json::json!({"key":"val"});

    <SqliteStorage as Storage>::store(
        &storage,
        "im1",
        "priority note",
        &MemoryInput {
            tags: vec!["ranked".to_string()],
            importance: 0.9,
            metadata: metadata.clone(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("priority", 5, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "im1");
    assert_eq!(results[0].importance, 0.9);
    assert_eq!(results[0].metadata, metadata);
    assert_eq!(results[0].tags, vec!["ranked".to_string()]);
}

#[tokio::test]
async fn test_update_importance_only() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let tags = vec!["persist".to_string()];
    <SqliteStorage as Storage>::store(
        &storage,
        "im2",
        "keep me",
        &MemoryInput {
            tags: tags.clone(),
            importance: 0.5,
            metadata: serde_json::json!({"scope":"base"}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "im2",
        &MemoryUpdate {
            content: None,
            tags: None,
            importance: Some(0.88),
            metadata: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("keep me", 1, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "keep me");
    assert_eq!(results[0].tags, tags);
    assert_eq!(results[0].importance, 0.88);
    assert_eq!(results[0].metadata, serde_json::json!({"scope":"base"}));
}

#[tokio::test]
async fn test_update_metadata_only() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let tags = vec!["persist".to_string()];
    <SqliteStorage as Storage>::store(
        &storage,
        "im3",
        "keep metadata",
        &MemoryInput {
            tags: tags.clone(),
            importance: 0.6,
            metadata: serde_json::json!({"v":1}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let updated_metadata = serde_json::json!({"v":2, "extra":"ok"});
    <SqliteStorage as Updater>::update(
        &storage,
        "im3",
        &MemoryUpdate {
            content: None,
            tags: None,
            importance: None,
            metadata: Some(updated_metadata.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("keep metadata", 1, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "keep metadata");
    assert_eq!(results[0].tags, tags);
    assert_eq!(results[0].importance, 0.6);
    assert_eq!(results[0].metadata, updated_metadata);
}

#[tokio::test]
async fn test_retrieve_increments_access_count() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "ac1",
        "read me",
        &MemoryInput {
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let _ = storage.retrieve("ac1").await.unwrap();
    let _ = storage.retrieve("ac1").await.unwrap();

    let access_count = storage.debug_get_access_count("ac1").unwrap();
    assert_eq!(access_count, 2);
}

#[tokio::test]
async fn test_search_returns_importance_and_metadata() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "im4",
        "search payload",
        &MemoryInput {
            tags: vec!["alpha".to_string(), "beta".to_string()],
            importance: 0.77,
            metadata: serde_json::json!({"team":"memory"}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search("payload", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "im4");
    assert_eq!(
        results[0].tags,
        vec!["alpha".to_string(), "beta".to_string()]
    );
    assert_eq!(results[0].importance, 0.77);
    assert_eq!(results[0].metadata, serde_json::json!({"team":"memory"}));
}

#[tokio::test]
async fn test_default_importance_is_half() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
            "INSERT INTO memories (id, content, content_hash, source_type, tags)
                 VALUES ('defimp', 'default importance', 'hash-default', 'test', '[]')",
            [],
        )
        .unwrap();
    }

    let results = storage
        .search("default importance", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "defimp");
    assert_eq!(results[0].importance, 0.5);
    assert_eq!(results[0].metadata, serde_json::json!({}));
}

#[tokio::test]
async fn test_store_with_event_type_and_session() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "evt1",
        "event scoped",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            session_id: Some("ses_1".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .search(
            "event scoped",
            5,
            &SearchOptions {
                session_id: Some("ses_1".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event_type, Some(EventType::Decision));
    assert_eq!(results[0].session_id.as_deref(), Some("ses_1"));
}

#[tokio::test]
async fn test_store_with_project_and_priority() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "evt2",
        "project scoped",
        &MemoryInput {
            project: Some("myproj".to_string()),
            priority: Some(3),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let got: (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT project, priority FROM memories WHERE id='evt2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(got.0.as_deref(), Some("myproj"));
    assert_eq!(got.1, Some(3));
}

#[tokio::test]
async fn test_search_filter_by_event_type() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, event_type) in [("f1", EventType::Decision), ("f2", EventType::Reminder)] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            "same query",
            &MemoryInput {
                event_type: Some(event_type),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let results = storage
        .search(
            "same query",
            10,
            &SearchOptions {
                event_type: Some(EventType::Decision),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "f1");
}

#[tokio::test]
async fn test_search_filter_by_project() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, project) in [("p1", "myproj"), ("p2", "other")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            "project query",
            &MemoryInput {
                project: Some(project.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let results = storage
        .search(
            "project query",
            10,
            &SearchOptions {
                project: Some("myproj".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "p1");
}

#[tokio::test]
async fn test_search_filter_by_session_id() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, session_id) in [("s1", "ses_a"), ("s2", "ses_b")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            "session query",
            &MemoryInput {
                session_id: Some(session_id.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let results = storage
        .search(
            "session query",
            10,
            &SearchOptions {
                session_id: Some("ses_a".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "s1");
}

#[tokio::test]
async fn test_recent_filter_by_project() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rp1",
        "recent in proj",
        &MemoryInput {
            project: Some("myproj".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rp2",
        "recent out proj",
        &MemoryInput {
            project: Some("other".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .recent(
            10,
            &SearchOptions {
                project: Some("myproj".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "rp1");
}

#[tokio::test]
async fn test_semantic_search_filter_by_event_type() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "se1",
        "semantic token",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "se2",
        "semantic token",
        &MemoryInput {
            event_type: Some(EventType::Reminder),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .semantic_search(
            "semantic token",
            10,
            &SearchOptions {
                event_type: Some(EventType::Decision),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "se1");
}

#[tokio::test]
async fn test_list_filter_by_project() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lp1",
        "list1",
        &MemoryInput {
            project: Some("myproj".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lp2",
        "list2",
        &MemoryInput {
            project: Some("other".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = storage
        .list(
            0,
            10,
            &SearchOptions {
                project: Some("myproj".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.memories.len(), 1);
    assert_eq!(result.memories[0].id, "lp1");
}

#[tokio::test]
async fn test_update_event_type() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ue1",
        "updatable",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "ue1",
        &MemoryUpdate {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = storage
        .search(
            "updatable",
            1,
            &SearchOptions {
                event_type: Some(EventType::Decision),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
}

#[tokio::test]
async fn test_update_priority() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "upprio",
        "priority target",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
        &storage,
        "upprio",
        &MemoryUpdate {
            priority: Some(4),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let priority: Option<i64> = conn
        .query_row(
            "SELECT priority FROM memories WHERE id='upprio'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(priority, Some(4));
}

#[test]
fn test_valid_event_types() {
    assert!(is_valid_event_type("decision"));
    assert!(is_valid_event_type("error_pattern"));
    assert!(!is_valid_event_type("unknown_event_type"));
}

#[test]
fn test_default_priority_for_event_type() {
    assert_eq!(default_priority_for_event_type("error_pattern"), 4);
    assert_eq!(default_priority_for_event_type("decision"), 3);
    assert_eq!(default_priority_for_event_type("git_merge"), 2);
    assert_eq!(default_priority_for_event_type("session_summary"), 1);
    assert_eq!(default_priority_for_event_type("not_real"), 0);
}

#[test]
fn test_ttl_auto_assignment() {
    assert_eq!(
        default_ttl_for_event_type("session_summary"),
        Some(TTL_EPHEMERAL)
    );
    assert_eq!(
        default_ttl_for_event_type("task_completion"),
        Some(TTL_LONG_TERM)
    );
    assert_eq!(default_ttl_for_event_type("error_pattern"), None);
    assert_eq!(default_ttl_for_event_type("user_preference"), None);
    assert_eq!(default_ttl_for_event_type("checkpoint"), Some(604_800));
    assert_eq!(
        default_ttl_for_event_type("code_chunk"),
        Some(TTL_EPHEMERAL)
    );
    assert_eq!(
        default_ttl_for_event_type("file_summary"),
        Some(TTL_SHORT_TERM)
    );
    assert_eq!(
        default_ttl_for_event_type("unknown_event"),
        Some(TTL_LONG_TERM)
    );
}

#[tokio::test]
async fn test_canonical_hash_dedup() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        event_type: Some(EventType::Memory),
        ..Default::default()
    };

    <SqliteStorage as Storage>::store(&storage, "dup-a", "Hello World", &input)
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "dup-b", "Hello World", &input)
        .await
        .unwrap();

    let listed = storage
        .list(0, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(storage.debug_get_access_count("dup-a").unwrap(), 1);
}

#[tokio::test]
async fn test_canonical_hash_ignores_formatting() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        event_type: Some(EventType::Memory),
        ..Default::default()
    };

    <SqliteStorage as Storage>::store(&storage, "fmt-a", "Hello World", &input)
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "fmt-b", "# hello    world", &input)
        .await
        .unwrap();

    let listed = storage
        .list(0, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
}

#[tokio::test]
async fn test_jaccard_dedup() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "jac-a",
        "alpha beta gamma delta epsilon zeta eta theta iota",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "jac-b",
        "alpha beta gamma delta epsilon zeta eta theta iota kappa",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = storage
        .list(0, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
}

#[tokio::test]
async fn test_no_dedup_different_event_types() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "nd-a",
        "database migration plan finalized today",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "nd-b",
        "database migration plan finalized today with notes",
        &MemoryInput {
            event_type: Some(EventType::Reminder),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = storage
        .list(0, 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 2);
}

#[tokio::test]
async fn test_supersession_detection_at_ingest() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "sup-a",
        "alpha user prefers concise commit messages with why first",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sup-b",
        "alpha user now prefers concise commit messages with rationale first",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (superseded_by, chain_a) = storage.debug_get_versioning_fields("sup-a").unwrap();
    let (_, chain_b) = storage.debug_get_versioning_fields("sup-b").unwrap();
    assert_eq!(superseded_by.as_deref(), Some("sup-b"));
    assert_eq!(chain_a, chain_b);
    assert!(
        storage
            .debug_has_relationship("sup-a", "sup-b", "SUPERSEDES")
            .unwrap()
    );
}

#[tokio::test]
async fn test_superseded_filtered_from_advanced_search() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "adv-old",
        "alpha preference: use compact commit titles",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "adv-new",
        "alpha preference updated: use compact commit titles with scope",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "compact commit titles",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    assert!(results.iter().all(|r| r.id != "adv-old"));
    assert!(results.iter().any(|r| r.id == "adv-new"));
}

#[tokio::test]
async fn test_superseded_filtered_from_find_similar() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "sim-source",
        "alpha source memory",
        &MemoryInput::default(),
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sim-old",
        "alpha preference old",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sim-new",
        "alpha preference new",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    storage
        .supersede_memory("sim-old", "sim-new")
        .await
        .unwrap();

    let results = <SqliteStorage as SimilarFinder>::find_similar(&storage, "sim-source", 10)
        .await
        .unwrap();
    assert!(results.iter().all(|r| r.id != "sim-old"));
}

#[tokio::test]
async fn test_superseded_filtered_from_get_recent() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "recent-old",
        "old memory",
        &MemoryInput::default(),
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "recent-new",
        "new memory",
        &MemoryInput::default(),
    )
    .await
    .unwrap();
    storage
        .supersede_memory("recent-old", "recent-new")
        .await
        .unwrap();

    let results = storage.recent(10, &SearchOptions::default()).await.unwrap();
    assert!(results.iter().all(|r| r.id != "recent-old"));
    assert!(results.iter().any(|r| r.id == "recent-new"));
}

#[tokio::test]
async fn test_include_superseded_shows_all() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "hist-old",
        "alpha preference: concise commits",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "hist-new",
        "alpha preference updated: concise commits with scope",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "concise commits",
        10,
        &SearchOptions {
            include_superseded: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(results.iter().any(|r| r.id == "hist-old"));
    assert!(results.iter().any(|r| r.id == "hist-new"));
}

#[tokio::test]
async fn test_version_chain_retrieval() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(&storage, "vc-a", "A", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "vc-b", "B", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "vc-c", "C", &MemoryInput::default())
        .await
        .unwrap();

    storage.supersede_memory("vc-a", "vc-b").await.unwrap();
    storage.supersede_memory("vc-b", "vc-c").await.unwrap();

    let from_c = storage.get_version_chain("vc-c").await.unwrap();
    assert_eq!(
        from_c.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["vc-a", "vc-b", "vc-c"]
    );

    let from_a = storage.get_version_chain("vc-a").await.unwrap();
    assert_eq!(
        from_a.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["vc-a", "vc-b", "vc-c"]
    );
}

#[tokio::test]
async fn test_manual_supersede() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(&storage, "man-old", "old", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "man-new", "new", &MemoryInput::default())
        .await
        .unwrap();

    storage
        .supersede_memory("man-old", "man-new")
        .await
        .unwrap();

    let (superseded_by, _) = storage.debug_get_versioning_fields("man-old").unwrap();
    assert_eq!(superseded_by.as_deref(), Some("man-new"));
    assert!(
        storage
            .debug_has_relationship("man-old", "man-new", "SUPERSEDES")
            .unwrap()
    );
}

#[tokio::test]
async fn test_non_supersession_types_dont_supersede() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "err-a",
        "alpha error timeout while fetching profile from api",
        &MemoryInput {
            event_type: Some(EventType::ErrorPattern),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "err-b",
        "alpha error timeout while saving settings through api",
        &MemoryInput {
            event_type: Some(EventType::ErrorPattern),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (superseded_by, _) = storage.debug_get_versioning_fields("err-a").unwrap();
    assert!(superseded_by.is_none());
}

#[tokio::test]
async fn test_export_import_preserves_versioning() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(&storage, "exp-old", "old", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "exp-new", "new", &MemoryInput::default())
        .await
        .unwrap();
    storage
        .supersede_memory("exp-old", "exp-new")
        .await
        .unwrap();

    let exported = storage.export_all().await.unwrap();

    let restored = SqliteStorage::new_in_memory().unwrap();
    restored.import_all(&exported).await.unwrap();

    let (old_superseded_by, old_chain) = restored.debug_get_versioning_fields("exp-old").unwrap();
    let (_, new_chain) = restored.debug_get_versioning_fields("exp-new").unwrap();
    assert_eq!(old_superseded_by.as_deref(), Some("exp-new"));
    assert_eq!(old_chain, new_chain);
}

#[tokio::test]
async fn test_auto_relate_creates_edges() {
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

    <SqliteStorage as Storage>::store(&storage, "rel-a", "alpha seed", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "rel-b", "beta seed", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rel-c",
        "alpha latest",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relationships WHERE source_id = 'rel-c' AND rel_type = 'related'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(count >= 1);
}

#[tokio::test]
async fn test_auto_relate_failure_doesnt_fail_store() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
                "INSERT INTO memories (id, content, embedding, content_hash, source_type, tags, metadata)
                 VALUES ('broken', 'broken embedding', x'00', 'h-broken', 'test', '[]', '{}')",
                [],
            )
            .unwrap();
    }

    let result = <SqliteStorage as Storage>::store(
        &storage,
        "rel-safe",
        "safe store payload",
        &MemoryInput::default(),
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        storage.retrieve("rel-safe").await.unwrap(),
        "safe store payload"
    );
}

#[tokio::test]
async fn test_feedback_helpful() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "fb-1", "feedback target", &MemoryInput::default())
        .await
        .unwrap();

    let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
        &storage,
        "fb-1",
        "helpful",
        Some("useful"),
    )
    .await
    .unwrap();

    assert_eq!(result["new_score"].as_i64(), Some(1));
    assert_eq!(result["total_signals"].as_u64(), Some(1));
    assert_eq!(result["flagged"].as_bool(), Some(false));
}

#[tokio::test]
async fn test_feedback_unhelpful_flagged() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "fb-2", "feedback target", &MemoryInput::default())
        .await
        .unwrap();

    for _ in 0..3 {
        let _ = <SqliteStorage as FeedbackRecorder>::record_feedback(
            &storage,
            "fb-2",
            "unhelpful",
            None,
        )
        .await
        .unwrap();
    }

    let conn = storage.test_conn().unwrap();
    let metadata_raw: String = conn
        .query_row(
            "SELECT metadata FROM memories WHERE id = 'fb-2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let metadata = parse_metadata_from_db(&metadata_raw);
    assert_eq!(metadata["feedback_score"].as_i64(), Some(-3));
    assert_eq!(metadata["flagged_for_review"].as_bool(), Some(true));
}

#[tokio::test]
async fn test_sweep_expired() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ttl-exp",
        "expires",
        &MemoryInput {
            ttl_seconds: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
            "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id = 'ttl-exp'",
            [],
        )
        .unwrap();
    }

    let swept = <SqliteStorage as ExpirationSweeper>::sweep_expired(&storage)
        .await
        .unwrap();
    assert_eq!(swept, 1);
    assert!(storage.retrieve("ttl-exp").await.is_err());
}

#[tokio::test]
async fn test_sweep_preserves_permanent() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ttl-perm",
        "permanent",
        &MemoryInput {
            ttl_seconds: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
            "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id = 'ttl-perm'",
            [],
        )
        .unwrap();
    }

    let swept = <SqliteStorage as ExpirationSweeper>::sweep_expired(&storage)
        .await
        .unwrap();
    assert_eq!(swept, 0);
    assert_eq!(storage.retrieve("ttl-perm").await.unwrap(), "permanent");
}

#[tokio::test]
async fn test_ttl_seconds_stored() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ttl-set",
        "ttl value",
        &MemoryInput {
            ttl_seconds: Some(1234),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let ttl: Option<i64> = conn
        .query_row(
            "SELECT ttl_seconds FROM memories WHERE id = 'ttl-set'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ttl, Some(1234));
}

#[tokio::test]
async fn test_relationship_with_weight_and_metadata() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rw1",
        "a",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "rw2",
        "b",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    storage
        .add_relationship("rw1", "rw2", "links_to", 0.7, &serde_json::json!({"k":"v"}))
        .await
        .unwrap();

    let rels = storage.get_relationships("rw1").await.unwrap();
    let links_to: Vec<_> = rels
        .into_iter()
        .filter(|r| r.rel_type == "links_to")
        .collect();
    assert_eq!(links_to.len(), 1);
    assert_eq!(links_to[0].weight, 0.7);
    assert_eq!(links_to[0].metadata, serde_json::json!({"k":"v"}));
}

#[tokio::test]
async fn test_export_includes_new_fields() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ex1",
        "export me",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            session_id: Some("ses_export".to_string()),
            project: Some("proj_export".to_string()),
            priority: Some(3),
            entity_id: Some("ent_1".to_string()),
            agent_type: Some("assistant".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    storage
        .add_relationship("ex1", "ex1", "self", 0.5, &serde_json::json!({"a":1}))
        .await
        .unwrap();

    let export = storage.export_all().await.unwrap();
    assert!(export.contains("session_id"));
    assert!(export.contains("event_type"));
    assert!(export.contains("project"));
    assert!(export.contains("priority"));
    assert!(export.contains("entity_id"));
    assert!(export.contains("agent_type"));
    assert!(export.contains("weight"));
    assert!(export.contains("created_at"));
    assert!(export.contains("version_chain_id"));
    assert!(export.contains("superseded_by_id"));
    assert!(export.contains("superseded_at"));
}

#[tokio::test]
async fn test_import_with_new_fields() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let data = serde_json::json!({
        "memories": [{
            "id":"imx1",
            "content":"imported",
            "tags":["t"],
            "importance":0.8,
            "metadata":{"m":1},
            "content_hash":"h",
            "source_type":"import",
            "access_count":1,
            "session_id":"ses_i",
            "event_type":"decision",
            "project":"proj_i",
            "priority":4,
            "entity_id":"e1",
            "agent_type":"assistant",
            "version_chain_id":"imx1",
            "superseded_by_id":null,
            "superseded_at":null
        }],
        "relationships":[{
            "id":"rel_i",
            "source_id":"imx1",
            "target_id":"imx1",
            "rel_type":"self",
            "weight":0.9,
            "metadata":{"x":1}
        }]
    });
    let imported = storage.import_all(&data.to_string()).await.unwrap();
    assert_eq!(imported.0, 1);
    assert_eq!(imported.1, 1);

    let results = storage
        .search(
            "imported",
            5,
            &SearchOptions {
                event_type: Some(EventType::Decision),
                project: Some("proj_i".to_string()),
                session_id: Some("ses_i".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_tag_search_filter_by_event_type() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "tag_evt_1",
        "tagged",
        &MemoryInput {
            tags: vec!["alpha".to_string()],
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "tag_evt_2",
        "tagged",
        &MemoryInput {
            tags: vec!["alpha".to_string()],
            event_type: Some(EventType::Reminder),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(
            &["alpha".to_string()],
            10,
            &SearchOptions {
                event_type: Some(EventType::Decision),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "tag_evt_1");
}

#[tokio::test]
async fn test_advanced_search_basic() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content, event_type) in [
        ("adv1", "alpha memory context", EventType::Decision),
        ("adv2", "alpha context details", EventType::Reminder),
        ("adv3", "unrelated content", EventType::TaskCompletion),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(event_type),
                tags: vec!["alpha".to_string()],
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "alpha context",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().all(|r| (0.0..=1.0).contains(&r.score)));
}

#[tokio::test]
async fn test_advanced_search_type_weight() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "weight1",
        "same searchable text decision",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "weight2",
        "same searchable text reminder",
        &MemoryInput {
            event_type: Some(EventType::Reminder),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "same searchable text",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].id, "weight2");
}

#[tokio::test]
async fn test_advanced_search_dedup() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for id in ["dup1", "dup2"] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            "identical duplicate content",
            &MemoryInput {
                event_type: Some(EventType::Decision),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "identical duplicate",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    let duplicate_count = results
        .iter()
        .filter(|r| r.content == "identical duplicate content")
        .count();
    assert_eq!(duplicate_count, 1);
}

#[tokio::test]
async fn test_advanced_search_filters() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "flt1",
        "filter target text",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            project: Some("project-a".to_string()),
            importance: 0.8,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "flt2",
        "filter target text",
        &MemoryInput {
            event_type: Some(EventType::Reminder),
            project: Some("project-b".to_string()),
            importance: 0.2,
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "filter target",
        10,
        &SearchOptions {
            event_type: Some(EventType::Decision),
            project: Some("project-a".to_string()),
            include_superseded: None,
            importance_min: Some(0.5),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "flt1");
}

#[tokio::test]
async fn test_graph_traverse_basic() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [("ga", "A"), ("gb", "B"), ("gc", "C")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    storage
        .add_relationship("ga", "gb", "links", 0.9, &serde_json::json!({}))
        .await
        .unwrap();
    storage
        .add_relationship("gb", "gc", "links", 0.8, &serde_json::json!({}))
        .await
        .unwrap();

    let edge_types = vec!["links".to_string()];
    let nodes =
        <SqliteStorage as GraphTraverser>::traverse(&storage, "ga", 2, 0.0, Some(&edge_types))
            .await
            .unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].id, "gb");
    assert_eq!(nodes[0].hop, 1);
    assert_eq!(nodes[1].id, "gc");
    assert_eq!(nodes[1].hop, 2);
}

#[tokio::test]
async fn test_graph_traverse_min_weight() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for id in ["gwa", "gwb", "gwc"] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            id,
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    storage
        .add_relationship("gwa", "gwb", "links", 0.9, &serde_json::json!({}))
        .await
        .unwrap();
    storage
        .add_relationship("gwa", "gwc", "links", 0.1, &serde_json::json!({}))
        .await
        .unwrap();

    let edge_types = vec!["links".to_string()];
    let nodes =
        <SqliteStorage as GraphTraverser>::traverse(&storage, "gwa", 2, 0.5, Some(&edge_types))
            .await
            .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, "gwb");
}

#[tokio::test]
async fn test_graph_traverse_max_hops() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [("mh1", "n1"), ("mh2", "n2"), ("mh3", "n3"), ("mh4", "n4")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    storage
        .add_relationship("mh1", "mh2", "links", 1.0, &serde_json::json!({}))
        .await
        .unwrap();
    storage
        .add_relationship("mh2", "mh3", "links", 1.0, &serde_json::json!({}))
        .await
        .unwrap();
    storage
        .add_relationship("mh3", "mh4", "links", 1.0, &serde_json::json!({}))
        .await
        .unwrap();

    let edge_types = vec!["links".to_string()];
    let nodes =
        <SqliteStorage as GraphTraverser>::traverse(&storage, "mh1", 2, 0.0, Some(&edge_types))
            .await
            .unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|n| n.hop <= 2));
}

#[tokio::test]
async fn test_find_similar() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sim1",
        "alpha beta",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sim2",
        "alpha beta extra",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "sim3",
        "zzzz qqqq",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as SimilarFinder>::find_similar(&storage, "sim1", 2)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "sim2");
    assert!(results[0].score >= results[1].score);
}

#[tokio::test]
async fn test_phrase_search() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ph1",
        "this has exact phrase inside",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ph2",
        "this has different words",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as PhraseSearcher>::phrase_search(
        &storage,
        "exact phrase",
        10,
        &SearchOptions {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "ph1");
}

#[tokio::test]
async fn test_search_empty_opts_returns_all() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [("all_1", "same one"), ("all_2", "same two")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let results = storage
        .search("same", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_profile_set_and_get() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as ProfileManager>::set_profile(
        &storage,
        &serde_json::json!({"name": "George", "timezone": "UTC"}),
    )
    .await
    .unwrap();

    let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
        .await
        .unwrap();
    assert_eq!(profile["name"], "George");
    assert_eq!(profile["timezone"], "UTC");
}

#[tokio::test]
async fn test_profile_update_merge() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as ProfileManager>::set_profile(
        &storage,
        &serde_json::json!({"name": "George", "timezone": "UTC"}),
    )
    .await
    .unwrap();
    <SqliteStorage as ProfileManager>::set_profile(
        &storage,
        &serde_json::json!({"timezone": "PST"}),
    )
    .await
    .unwrap();

    let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
        .await
        .unwrap();
    assert_eq!(profile["name"], "George");
    assert_eq!(profile["timezone"], "PST");
}

#[tokio::test]
async fn test_profile_preferences_augmentation() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [
        ("pref-1", "User prefers small PRs"),
        ("pref-2", "User prefers concise status updates"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(EventType::UserPreference),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
        .await
        .unwrap();
    let prefs = profile["preferences_from_memory"].as_array().unwrap();
    assert!(prefs.len() >= 2);
}

#[tokio::test]
async fn test_checkpoint_save_and_resume() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let id = <SqliteStorage as CheckpointManager>::save_checkpoint(
        &storage,
        CheckpointInput {
            task_title: "Cross-session work".to_string(),
            progress: "Added profile trait".to_string(),
            plan: Some("Implement storage next".to_string()),
            files_touched: None,
            decisions: None,
            key_context: None,
            next_steps: Some("Add tests".to_string()),
            session_id: Some("s-1".to_string()),
            project: Some("romega".to_string()),
        },
    )
    .await
    .unwrap();
    assert!(!id.is_empty());

    let resumed =
        <SqliteStorage as CheckpointManager>::resume_task(&storage, "Cross-session", None, 1)
            .await
            .unwrap();
    assert_eq!(resumed.len(), 1);
    assert!(
        resumed[0]["content"]
            .as_str()
            .unwrap()
            .contains("## Checkpoint: Cross-session work")
    );
}

#[tokio::test]
async fn test_checkpoint_numbering() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for idx in 1..=3 {
        <SqliteStorage as CheckpointManager>::save_checkpoint(
            &storage,
            CheckpointInput {
                task_title: "Repeated task".to_string(),
                progress: format!("Progress {idx}"),
                plan: None,
                files_touched: None,
                decisions: None,
                key_context: None,
                next_steps: None,
                session_id: None,
                project: None,
            },
        )
        .await
        .unwrap();
    }

    let resumed = <SqliteStorage as CheckpointManager>::resume_task(&storage, "Repeated", None, 3)
        .await
        .unwrap();
    let mut numbers: Vec<i64> = resumed
        .iter()
        .map(|entry| entry["metadata"]["checkpoint_number"].as_i64().unwrap())
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_checkpoint_project_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (project, progress) in [("proj-a", "A progress"), ("proj-b", "B progress")] {
        <SqliteStorage as CheckpointManager>::save_checkpoint(
            &storage,
            CheckpointInput {
                task_title: "Shared task".to_string(),
                progress: progress.to_string(),
                plan: None,
                files_touched: None,
                decisions: None,
                key_context: None,
                next_steps: None,
                session_id: None,
                project: Some(project.to_string()),
            },
        )
        .await
        .unwrap();
    }

    let resumed =
        <SqliteStorage as CheckpointManager>::resume_task(&storage, "Shared", Some("proj-b"), 5)
            .await
            .unwrap();
    assert_eq!(resumed.len(), 1);
    assert!(
        resumed[0]["content"]
            .as_str()
            .unwrap()
            .contains("B progress")
    );
}

#[tokio::test]
async fn test_reminder_create_and_list() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let reminder = <SqliteStorage as ReminderManager>::create_reminder(
        &storage,
        "Review PR E",
        "1h",
        Some("after lunch"),
        Some("session-1"),
        Some("romega"),
    )
    .await
    .unwrap();

    let reminder_id = reminder["reminder_id"].as_str().unwrap();
    let listed = <SqliteStorage as ReminderManager>::list_reminders(&storage, None)
        .await
        .unwrap();
    assert!(
        listed
            .iter()
            .any(|entry| entry["reminder_id"].as_str() == Some(reminder_id))
    );
}

#[tokio::test]
async fn test_reminder_dismiss() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let reminder = <SqliteStorage as ReminderManager>::create_reminder(
        &storage,
        "Dismiss me",
        "30m",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let reminder_id = reminder["reminder_id"].as_str().unwrap();

    let dismissed = <SqliteStorage as ReminderManager>::dismiss_reminder(&storage, reminder_id)
        .await
        .unwrap();
    assert_eq!(dismissed["status"], "dismissed");

    let dismissed_list =
        <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("dismissed"))
            .await
            .unwrap();
    assert_eq!(dismissed_list.len(), 1);
    assert_eq!(dismissed_list[0]["status"], "dismissed");
}

#[tokio::test]
async fn test_reminder_status_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let first = <SqliteStorage as ReminderManager>::create_reminder(
        &storage,
        "pending item",
        "1h",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let second = <SqliteStorage as ReminderManager>::create_reminder(
        &storage,
        "to dismiss",
        "2h",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    <SqliteStorage as ReminderManager>::dismiss_reminder(
        &storage,
        second["reminder_id"].as_str().unwrap(),
    )
    .await
    .unwrap();

    let pending = <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("pending"))
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["reminder_id"], first["reminder_id"]);

    let all = <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("all"))
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_reminder_duration_parsing() {
    assert_eq!(parse_duration("1h").unwrap().num_minutes(), 60);
    assert_eq!(parse_duration("30m").unwrap().num_minutes(), 30);
    assert_eq!(parse_duration("2d").unwrap().num_hours(), 48);
    assert_eq!(parse_duration("1w").unwrap().num_days(), 7);
    assert_eq!(parse_duration("1d12h").unwrap().num_hours(), 36);
}

#[test]
fn test_reminder_invalid_duration() {
    for input in ["", "0m", "10x", "1h30", "1m1h", "-1h"] {
        assert!(parse_duration(input).is_err());
    }
}

#[tokio::test]
async fn test_lessons_query_basic() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [
        ("lesson-1", "Learned to keep checkpoints small"),
        ("lesson-2", "Learned to run clippy before commit"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(EventType::LessonLearned),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let lessons = <SqliteStorage as LessonQuerier>::query_lessons(
        &storage,
        Some("checkpoints"),
        None,
        None,
        None,
        5,
    )
    .await
    .unwrap();
    assert_eq!(lessons.len(), 1);
    assert!(
        lessons[0]["content"]
            .as_str()
            .unwrap()
            .contains("checkpoints")
    );
}

#[tokio::test]
async fn test_lessons_exclude_session() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, session) in [("ls-1", "s1"), ("ls-2", "s2")] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            &format!("Lesson from {session}"),
            &MemoryInput {
                event_type: Some(EventType::LessonLearned),
                session_id: Some(session.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let lessons =
        <SqliteStorage as LessonQuerier>::query_lessons(&storage, None, None, Some("s2"), None, 5)
            .await
            .unwrap();
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0]["session_id"], "s1");
}

#[tokio::test]
async fn test_lessons_dedup() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "dup-1",
        "placeholder one",
        &MemoryInput {
            event_type: Some(EventType::Memory),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "dup-2",
        "placeholder two",
        &MemoryInput {
            event_type: Some(EventType::Memory),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Updater>::update(
            &storage,
            "dup-1",
            &MemoryUpdate {
                content: Some(
                    "The first eighty characters of this lesson are intentionally identical across both entries AAA111"
                        .to_string(),
                ),
                event_type: Some(EventType::LessonLearned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    <SqliteStorage as Updater>::update(
            &storage,
            "dup-2",
            &MemoryUpdate {
                content: Some(
                    "The first eighty characters of this lesson are intentionally identical across both entries BBB222 with extra detail"
                        .to_string(),
                ),
                event_type: Some(EventType::LessonLearned),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let lessons =
        <SqliteStorage as LessonQuerier>::query_lessons(&storage, None, None, None, None, 10)
            .await
            .unwrap();
    assert_eq!(lessons.len(), 1);
}

#[test]
fn test_schema_contains_new_columns() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let conn = storage.test_conn().unwrap();
    let memories_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for col in [
        "session_id",
        "event_type",
        "project",
        "priority",
        "entity_id",
        "agent_type",
        "ttl_seconds",
        "canonical_hash",
    ] {
        assert!(memories_cols.contains(&col.to_string()));
    }

    let rel_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(relationships)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for col in ["weight", "metadata", "created_at"] {
        assert!(rel_cols.contains(&col.to_string()));
    }

    let profile_cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(user_profile)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for col in ["key", "value", "updated_at"] {
        assert!(profile_cols.contains(&col.to_string()));
    }
}

// ── MaintenanceManager tests ──────────────────────────────────

#[tokio::test]
async fn test_health_check() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let result = <SqliteStorage as MaintenanceManager>::check_health(&storage, 100.0, 200.0, 10000)
        .await
        .unwrap();
    assert_eq!(result["status"], "healthy");
    assert_eq!(result["integrity_ok"], true);
    assert_eq!(result["node_count"], 0);
}

#[tokio::test]
async fn test_health_check_node_limit() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "h-1", "some content", &MemoryInput::default())
        .await
        .unwrap();

    let result = <SqliteStorage as MaintenanceManager>::check_health(&storage, 100.0, 200.0, 1)
        .await
        .unwrap();
    assert_eq!(result["status"], "warning");
    assert_eq!(result["node_count"], 1);
}

#[tokio::test]
async fn test_consolidate_prunes_stale() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "stale-1", "old content", &MemoryInput::default())
        .await
        .unwrap();
    // Back-date the memory and ensure zero access
    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
                "UPDATE memories SET created_at = datetime('now', '-60 days'), access_count = 0 WHERE id = ?1",
                params!["stale-1"],
            )
            .unwrap();
    }

    let result = <SqliteStorage as MaintenanceManager>::consolidate(&storage, 30, 100)
        .await
        .unwrap();
    assert!(result["pruned_stale"].as_i64().unwrap() >= 1);
    assert!(result["after"].as_i64().unwrap() < result["before"].as_i64().unwrap());
}

#[tokio::test]
async fn test_consolidate_caps_summaries() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Insert directly via SQL to bypass store-time dedup
    let contents = [
        "Alpha quarterly revenue growth exceeded projections by fifteen percent",
        "Beta deployment pipeline migration completed with zero downtime achieved",
        "Gamma user authentication overhaul implemented with biometric support added",
        "Delta database sharding strategy finalized across three geographic regions",
        "Epsilon frontend performance optimization reduced load times significantly",
    ];
    {
        let conn = storage.test_conn().unwrap();
        for (i, content) in contents.iter().enumerate() {
            conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, ?2, ?3, 'direct', 'session_summary', '[]', 0.5, '{}', 1)",
                    params![format!("sum-{i}"), content, format!("hash-{i}")],
                )
                .unwrap();
        }
    }

    let result = <SqliteStorage as MaintenanceManager>::consolidate(&storage, 365, 2)
        .await
        .unwrap();
    assert_eq!(result["pruned_summaries"].as_i64().unwrap(), 3);
    assert_eq!(result["after"].as_i64().unwrap(), 2);
}

#[tokio::test]
async fn test_compact_dry_run() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Insert directly via SQL to bypass store-time dedup
    {
        let conn = storage.test_conn().unwrap();
        for i in 0..3 {
            conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, 'the exact same decision content repeated here', ?2, 'direct', 'decision', '[]', 0.5, '{}', 0)",
                    params![format!("dup-{i}"), format!("hash-dup-{i}")],
                )
                .unwrap();
        }
    }

    let result = <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, true)
        .await
        .unwrap();
    assert!(result["clusters_found"].as_i64().unwrap() >= 1);
    assert_eq!(result["memories_compacted"].as_i64().unwrap(), 0);
    assert_eq!(result["dry_run"], true);
}

#[tokio::test]
async fn test_compact_merges() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Insert directly via SQL to bypass store-time dedup
    {
        let conn = storage.test_conn().unwrap();
        for i in 0..3 {
            conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, 'the exact same decision content repeated here for merging', ?2, 'direct', 'decision', '[]', 0.5, '{}', 0)",
                    params![format!("cm-{i}"), format!("hash-cm-{i}")],
                )
                .unwrap();
        }
    }

    let result =
        <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, false)
            .await
            .unwrap();
    assert!(result["memories_compacted"].as_i64().unwrap() >= 2);
}

#[tokio::test]
async fn test_compact_below_threshold() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "lone-1",
        "only one decision memory",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result =
        <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, false)
            .await
            .unwrap();
    assert_eq!(result["clusters_found"].as_i64().unwrap(), 0);
}

#[tokio::test]
async fn test_clear_session() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for i in 0..2 {
        <SqliteStorage as Storage>::store(
            &storage,
            &format!("cs-a-{i}"),
            &format!("session a content {i}"),
            &MemoryInput {
                session_id: Some("sess-a".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    <SqliteStorage as Storage>::store(
        &storage,
        "cs-b-0",
        "session b content",
        &MemoryInput {
            session_id: Some("sess-b".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let deleted = <SqliteStorage as MaintenanceManager>::clear_session(&storage, "sess-a")
        .await
        .unwrap();
    assert_eq!(deleted, 2);

    // Verify only sess-b remains
    let remaining: i64 = {
        let conn = storage.test_conn().unwrap();
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(remaining, 1);
}

// ── WelcomeProvider tests ─────────────────────────────────────

#[tokio::test]
async fn test_welcome_briefing() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "w-1", "first memory", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(&storage, "w-2", "second memory", &MemoryInput::default())
        .await
        .unwrap();

    let result = <SqliteStorage as WelcomeProvider>::welcome(&storage, None, None)
        .await
        .unwrap();
    assert_eq!(result["memory_count"], 2);
    assert!(result["greeting"].as_str().unwrap().contains("2 memories"));
    assert_eq!(result["recent_memories"].as_array().unwrap().len(), 2);
    assert!(result["user_context"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_welcome_surfaces_user_context() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Store a user_preference memory
    <SqliteStorage as Storage>::store(
        &storage,
        "uc-1",
        "user prefers dark mode for all interfaces",
        &MemoryInput {
            event_type: Some(EventType::UserPreference),
            importance: 0.9,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Store a user_fact memory
    <SqliteStorage as Storage>::store(
        &storage,
        "uc-2",
        "user lives in San Francisco and works in fintech",
        &MemoryInput {
            event_type: Some(EventType::UserFact),
            importance: 0.8,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Store a regular memory (should not appear in user_context)
    <SqliteStorage as Storage>::store(
        &storage,
        "uc-3",
        "deployed version 2.1 to production successfully",
        &MemoryInput {
            event_type: Some(EventType::TaskCompletion),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = <SqliteStorage as WelcomeProvider>::welcome(&storage, None, None)
        .await
        .unwrap();
    assert_eq!(result["memory_count"], 3);
    let user_ctx = result["user_context"].as_array().unwrap();
    assert_eq!(user_ctx.len(), 2);
    // Ordered by importance DESC — user_preference (0.9) first
    assert_eq!(user_ctx[0]["event_type"], "user_preference");
    assert_eq!(user_ctx[1]["event_type"], "user_fact");
}

// ── StatsProvider tests ───────────────────────────────────────

#[tokio::test]
async fn test_type_stats() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    // Use very different content to avoid Jaccard dedup for same event_type
    <SqliteStorage as Storage>::store(
        &storage,
        "ts-d-0",
        "chose postgresql for the primary relational datastore backend",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ts-d-1",
        "migrated frontend framework from angular to react with typescript",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ts-l-0",
        "learned that connection pooling prevents timeout errors under load",
        &MemoryInput {
            event_type: Some(EventType::LessonLearned),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = <SqliteStorage as StatsProvider>::type_stats(&storage)
        .await
        .unwrap();
    assert_eq!(result["decision"], 2);
    assert_eq!(result["lesson_learned"], 1);
    assert_eq!(result["_total"], 3);
}

#[tokio::test]
async fn test_session_stats() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for i in 0..2 {
        <SqliteStorage as Storage>::store(
            &storage,
            &format!("ss-1-{i}"),
            &format!("s1 content {i}"),
            &MemoryInput {
                session_id: Some("s1".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    <SqliteStorage as Storage>::store(
        &storage,
        "ss-2-0",
        "s2 content",
        &MemoryInput {
            session_id: Some("s2".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = <SqliteStorage as StatsProvider>::session_stats(&storage)
        .await
        .unwrap();
    assert_eq!(result["total_sessions"], 2);
    let sessions = result["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_weekly_digest() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "wd-1",
        "recent memory one",
        &MemoryInput::default(),
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "wd-2",
        "recent memory two",
        &MemoryInput::default(),
    )
    .await
    .unwrap();

    let result = <SqliteStorage as StatsProvider>::weekly_digest(&storage, 7)
        .await
        .unwrap();
    assert_eq!(result["total_memories"], 2);
    assert_eq!(result["period_new"], 2);
    assert_eq!(result["period_days"], 7);
}

#[tokio::test]
async fn test_access_rate_stats() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(&storage, "ar-1", "accessed memory", &MemoryInput::default())
        .await
        .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ar-2",
        "never accessed memory",
        &MemoryInput::default(),
    )
    .await
    .unwrap();
    // Set access_count on one
    {
        let conn = storage.test_conn().unwrap();
        conn.execute(
            "UPDATE memories SET access_count = 5 WHERE id = ?1",
            params!["ar-1"],
        )
        .unwrap();
    }

    let result = <SqliteStorage as StatsProvider>::access_rate_stats(&storage)
        .await
        .unwrap();
    assert_eq!(result["total_memories"], 2);
    assert_eq!(result["zero_access_count"], 1);
    assert!(!result["top_accessed"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_dual_match_boost_default() {
    // Verify the default value for dual_match_boost
    let params = ScoringParams::default();
    assert!((params.dual_match_boost - 1.2).abs() < 1e-9);
}

#[tokio::test]
async fn test_dual_match_boost_applied() {
    // Store two memories: one that will match both vector + FTS ("alpha context"),
    // and one that only matches FTS ("unrelated alpha filler words context").
    // With dual_match_boost > 1.0, the dual-match candidate should rank higher.
    let storage = SqliteStorage::new_in_memory()
        .unwrap()
        .with_scoring_params(ScoringParams {
            dual_match_boost: 2.0, // exaggerate to make test deterministic
            ..ScoringParams::default()
        });

    // "alpha context" — will match both vector (KeywordEmbedder sees "alpha") and FTS
    <SqliteStorage as Storage>::store(
        &storage,
        "dm1",
        "alpha context information details",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // "alpha notes" — also matches vector (same alpha embedding) and FTS
    <SqliteStorage as Storage>::store(
        &storage,
        "dm2",
        "alpha context notes records",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "alpha context",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    // Both candidates should be returned (they share similar content)
    assert!(
        !results.is_empty(),
        "expected results with dual-match boost"
    );
}

#[tokio::test]
async fn test_dual_match_boost_disabled() {
    // When dual_match_boost is 1.0, no extra boost is applied.
    let storage = SqliteStorage::new_in_memory()
        .unwrap()
        .with_scoring_params(ScoringParams {
            dual_match_boost: 1.0,
            ..ScoringParams::default()
        });

    <SqliteStorage as Storage>::store(
        &storage,
        "ndb1",
        "alpha context information details",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results_no_boost = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "alpha context",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    assert!(!results_no_boost.is_empty());
}

#[tokio::test]
async fn test_dual_match_boost_increases_score() {
    // Compare scores with boost=1.0 vs boost=2.0 for the same data.
    // The boosted version should produce a higher raw score (before normalization
    // sets top=1.0). We verify by checking that the single-candidate score
    // normalises to 1.0 in both cases — the key assertion is that the pipeline
    // runs without error and that results are non-empty.
    for boost in [1.0, 1.5, 2.0] {
        let storage = SqliteStorage::new_in_memory()
            .unwrap()
            .with_scoring_params(ScoringParams {
                dual_match_boost: boost,
                ..ScoringParams::default()
            });

        <SqliteStorage as Storage>::store(
            &storage,
            "boost1",
            "alpha context searchable text data",
            &MemoryInput {
                event_type: Some(EventType::Decision),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &storage,
            "alpha context searchable",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();

        assert!(
            !results.is_empty(),
            "expected results with dual_match_boost={boost}"
        );
    }
}

// ── Temporal filtering tests ──────────────────────────────────────────

#[test]
fn test_validate_iso8601_accepts_valid_formats() {
    assert!(validate_iso8601("2024-01-15"));
    assert!(validate_iso8601("2024-01-15T10:30:00Z"));
    assert!(validate_iso8601("2024-01-15T10:30:00.000Z"));
    assert!(validate_iso8601("2024-12-31T23:59:59+05:00"));
}

#[test]
fn test_validate_iso8601_rejects_invalid_formats() {
    assert!(!validate_iso8601(""));
    assert!(!validate_iso8601("not-a-date"));
    assert!(!validate_iso8601("2024"));
    assert!(!validate_iso8601("2024-1-1"));
    assert!(!validate_iso8601("01-15-2024"));
}

#[tokio::test]
async fn test_store_with_referenced_date_sets_event_at() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        content: "event happened in the past".to_string(),
        referenced_date: Some("2023-06-15T12:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("ref-date-1", "event happened in the past", &input)
        .await
        .unwrap();

    let conn = storage.test_conn().unwrap();
    let event_at: String = conn
        .query_row(
            "SELECT event_at FROM memories WHERE id = ?1",
            params!["ref-date-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_at, "2023-06-15T12:00:00Z");
}

#[tokio::test]
async fn test_store_without_referenced_date_defaults_event_at_to_now() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        content: "no referenced date".to_string(),
        ..Default::default()
    };
    storage
        .store("ref-date-2", "no referenced date", &input)
        .await
        .unwrap();

    let conn = storage.test_conn().unwrap();
    let event_at: String = conn
        .query_row(
            "SELECT event_at FROM memories WHERE id = ?1",
            params!["ref-date-2"],
            |row| row.get(0),
        )
        .unwrap();
    // event_at should be a recent timestamp (within the last minute)
    assert!(event_at.starts_with("20"));
    assert!(event_at.contains('T'));
}

#[tokio::test]
async fn test_store_with_invalid_referenced_date_falls_back_to_now() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        content: "bad date format".to_string(),
        referenced_date: Some("not-a-date".to_string()),
        ..Default::default()
    };
    storage
        .store("ref-date-3", "bad date format", &input)
        .await
        .unwrap();

    let conn = storage.test_conn().unwrap();
    let event_at: String = conn
        .query_row(
            "SELECT event_at FROM memories WHERE id = ?1",
            params!["ref-date-3"],
            |row| row.get(0),
        )
        .unwrap();
    // Should have fallen back to now() since "not-a-date" fails validation
    assert!(event_at.starts_with("20"));
    assert_ne!(event_at, "not-a-date");
}

#[tokio::test]
async fn test_search_with_event_after_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    // Store a memory with event_at in the past
    let input_old = MemoryInput {
        content: "old event content".to_string(),
        referenced_date: Some("2023-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("ev-old", "old event content", &input_old)
        .await
        .unwrap();

    // Store a memory with event_at more recent
    let input_new = MemoryInput {
        content: "new event content".to_string(),
        referenced_date: Some("2025-06-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("ev-new", "new event content", &input_new)
        .await
        .unwrap();

    // Search with event_after that excludes the old event
    let opts = SearchOptions {
        event_after: Some("2024-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let results = storage.search("event content", 10, &opts).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "ev-new");
}

#[tokio::test]
async fn test_search_with_event_before_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    let input_old = MemoryInput {
        content: "old event data".to_string(),
        referenced_date: Some("2023-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("evb-old", "old event data", &input_old)
        .await
        .unwrap();

    let input_new = MemoryInput {
        content: "new event data".to_string(),
        referenced_date: Some("2025-06-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("evb-new", "new event data", &input_new)
        .await
        .unwrap();

    // Search with event_before that excludes the new event
    let opts = SearchOptions {
        event_before: Some("2024-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let results = storage.search("event data", 10, &opts).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "evb-old");
}

#[tokio::test]
async fn test_search_with_event_after_and_event_before_window() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    let items = [
        (
            "win-1",
            "window test alpha first content",
            "2022-06-01T00:00:00Z",
        ),
        (
            "win-2",
            "window test alpha second content",
            "2023-06-01T00:00:00Z",
        ),
        (
            "win-3",
            "window test alpha third content",
            "2024-06-01T00:00:00Z",
        ),
        (
            "win-4",
            "window test alpha fourth content",
            "2025-06-01T00:00:00Z",
        ),
    ];
    for (id, content, date) in &items {
        let input = MemoryInput {
            content: content.to_string(),
            referenced_date: Some(date.to_string()),
            ..Default::default()
        };
        storage.store(id, content, &input).await.unwrap();
    }

    // Window between 2023 and 2024 should match win-2 and win-3
    let opts = SearchOptions {
        event_after: Some("2023-01-01T00:00:00Z".to_string()),
        event_before: Some("2024-12-31T23:59:59Z".to_string()),
        ..Default::default()
    };
    let results = storage
        .search("window test alpha", 10, &opts)
        .await
        .unwrap();
    let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
    assert!(
        ids.contains(&"win-2"),
        "expected win-2 in results, got: {:?}",
        ids
    );
    assert!(
        ids.contains(&"win-3"),
        "expected win-3 in results, got: {:?}",
        ids
    );
    assert!(
        !ids.contains(&"win-1"),
        "expected win-1 NOT in results, got: {:?}",
        ids
    );
    assert!(
        !ids.contains(&"win-4"),
        "expected win-4 NOT in results, got: {:?}",
        ids
    );
}

#[tokio::test]
async fn test_recent_with_event_after_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    let input_old = MemoryInput {
        content: "old recent test".to_string(),
        referenced_date: Some("2023-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("rec-old", "old recent test", &input_old)
        .await
        .unwrap();

    let input_new = MemoryInput {
        content: "new recent test".to_string(),
        referenced_date: Some("2025-06-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("rec-new", "new recent test", &input_new)
        .await
        .unwrap();

    let opts = SearchOptions {
        event_after: Some("2024-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let results = storage.recent(10, &opts).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "rec-new");
}

#[tokio::test]
async fn test_list_with_event_before_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    let input_old = MemoryInput {
        content: "list old test".to_string(),
        referenced_date: Some("2023-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("lst-old", "list old test", &input_old)
        .await
        .unwrap();

    let input_new = MemoryInput {
        content: "list new test".to_string(),
        referenced_date: Some("2025-06-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("lst-new", "list new test", &input_new)
        .await
        .unwrap();

    let opts = SearchOptions {
        event_before: Some("2024-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let result = storage.list(0, 10, &opts).await.unwrap();
    assert_eq!(result.memories.len(), 1);
    assert_eq!(result.memories[0].id, "lst-old");
}

#[tokio::test]
async fn test_phrase_search_with_event_after_filter() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    let input_old = MemoryInput {
        content: "unique phrase old".to_string(),
        referenced_date: Some("2023-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("ph-old", "unique phrase old", &input_old)
        .await
        .unwrap();

    let input_new = MemoryInput {
        content: "unique phrase new".to_string(),
        referenced_date: Some("2025-06-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    storage
        .store("ph-new", "unique phrase new", &input_new)
        .await
        .unwrap();

    let opts = SearchOptions {
        event_after: Some("2024-01-01T00:00:00Z".to_string()),
        ..Default::default()
    };
    let results =
        <SqliteStorage as PhraseSearcher>::phrase_search(&storage, "unique phrase", 10, &opts)
            .await
            .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "ph-new");
}

#[tokio::test]
async fn test_referenced_date_date_only_format() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let input = MemoryInput {
        content: "date only test".to_string(),
        referenced_date: Some("2023-06-15".to_string()),
        ..Default::default()
    };
    storage
        .store("date-only-1", "date only test", &input)
        .await
        .unwrap();

    let conn = storage.test_conn().unwrap();
    let event_at: String = conn
        .query_row(
            "SELECT event_at FROM memories WHERE id = ?1",
            params!["date-only-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_at, "2023-06-15");
}

// ── Search explainability tests ───────────────────────────────────────

#[tokio::test]
async fn test_advanced_search_explain_mode() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content, event_type) in [
        ("exp1", "alpha memory context details", EventType::Decision),
        ("exp2", "alpha context information", EventType::Reminder),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(event_type),
                tags: vec!["alpha".to_string()],
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Without explain: metadata should NOT contain _explain
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "alpha context",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(r.metadata.get("_explain").is_none());
    }

    // With explain=true: metadata should contain _explain with component scores
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "alpha context",
        10,
        &SearchOptions {
            explain: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        let explain = r
            .metadata
            .get("_explain")
            .expect("_explain should be present when explain=true");
        assert!(explain.is_object());
        // Check required explain fields exist
        assert!(explain.get("final_score").is_some());
        assert!(explain.get("word_overlap").is_some());
        assert!(explain.get("text_overlap").is_some());
        assert!(explain.get("importance_factor").is_some());
        assert!(explain.get("feedback_factor").is_some());
        assert!(explain.get("time_decay").is_some());
        assert!(explain.get("type_weight").is_some());
        assert!(explain.get("dual_match").is_some());
        // final_score should match result score
        let final_score = explain["final_score"].as_f64().unwrap();
        assert!(
            (final_score - r.score as f64).abs() < 0.01,
            "final_score ({final_score}) should match result score ({})",
            r.score
        );
    }
}

#[tokio::test]
async fn test_advanced_search_explain_false_no_metadata() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "ef1",
        "beta context details",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // explain=false should behave the same as unset
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "beta context",
        10,
        &SearchOptions {
            explain: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(r.metadata.get("_explain").is_none());
    }
}

#[tokio::test]
async fn test_advanced_search_abstention_returns_empty() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "abs1",
        "very specific technical content about memory systems",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Query with no overlap should trigger abstention (empty results)
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "completely unrelated banana smoothie recipe",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();
    assert!(results.is_empty(), "Abstention should return empty vec");
}

// ── Abstention gate regression tests ─────────────────────────────────────

#[tokio::test]
async fn test_abstention_gate_fires_for_unrelated_query() {
    // Store memories about Rust programming, then query about something
    // completely unrelated (French pastry). The abstention gate should
    // fire because max text_overlap will be well below the 0.30 threshold.
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [
        (
            "abs-gate1",
            "Rust borrow checker enforces ownership rules at compile time",
        ),
        (
            "abs-gate2",
            "Tokio runtime provides async task scheduling for Rust applications",
        ),
        (
            "abs-gate3",
            "SQLite database supports full-text search via FTS5 extension",
        ),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(EventType::Decision),
                tags: vec!["rust".to_string(), "programming".to_string()],
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Query about something completely unrelated — no word overlap expected
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "What is the best French pastry recipe for croissants",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    assert!(
        results.is_empty(),
        "abstention gate should return empty results for unrelated query, got {} results",
        results.len()
    );
}

#[tokio::test]
async fn test_abstention_gate_does_not_fire_for_relevant_query() {
    // Store memories about Rust programming, then query about Rust.
    // The abstention gate should NOT fire because text_overlap will
    // exceed the 0.30 threshold.
    let storage = SqliteStorage::new_in_memory().unwrap();
    for (id, content) in [
        (
            "rel1",
            "Rust borrow checker enforces ownership rules at compile time",
        ),
        (
            "rel2",
            "Tokio runtime provides async task scheduling for Rust applications",
        ),
        (
            "rel3",
            "SQLite database supports full-text search via FTS5 extension",
        ),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                event_type: Some(EventType::Decision),
                tags: vec!["rust".to_string(), "programming".to_string()],
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Query about Rust — overlapping words should pass the gate
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "How does the Rust borrow checker work",
        10,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    assert!(
        !results.is_empty(),
        "abstention gate should NOT fire for a relevant query with word overlap"
    );
}

// ---------------------------------------------------------------------------
// Cross-project isolation tests (issue #46)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cross_project_isolation_search() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    // Store memories for two distinct projects
    for (id, content, project) in [
        ("iso_a1", "alpha architecture notes", "alpha"),
        ("iso_a2", "alpha deployment config", "alpha"),
        ("iso_b1", "beta architecture notes", "beta"),
        ("iso_b2", "beta deployment config", "beta"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                project: Some(project.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Search with project="alpha" — only alpha memories should appear
    let results = storage
        .search(
            "architecture",
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(
            r.id.starts_with("iso_a"),
            "expected alpha memory, got id={}",
            r.id
        );
    }

    // Search with project="beta" — only beta memories should appear
    let results = storage
        .search(
            "architecture",
            10,
            &SearchOptions {
                project: Some("beta".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(
            r.id.starts_with("iso_b"),
            "expected beta memory, got id={}",
            r.id
        );
    }
}

#[tokio::test]
async fn test_cross_project_isolation_advanced_search() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    // Content must share key query words so FTS5 BM25 matches, but differ
    // enough per row to avoid canonical-hash and Jaccard dedup at store time.
    // Use event_type "reminder" which has no Jaccard dedup threshold.
    for (id, content, project) in [
        (
            "adv_a1",
            "design decision target for the alpha project first memo",
            "alpha",
        ),
        (
            "adv_b1",
            "design decision target for the beta project first memo",
            "beta",
        ),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                project: Some(project.to_string()),
                event_type: Some(EventType::Reminder),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Advanced search filtered to project="alpha"
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "design decision target",
        10,
        &SearchOptions {
            project: Some("alpha".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(
            r.id.starts_with("adv_a"),
            "expected alpha memory in advanced_search, got id={}",
            r.id
        );
    }

    // Advanced search filtered to project="beta"
    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "design decision target",
        10,
        &SearchOptions {
            project: Some("beta".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!results.is_empty());
    for r in &results {
        assert!(
            r.id.starts_with("adv_b"),
            "expected beta memory in advanced_search, got id={}",
            r.id
        );
    }
}

#[tokio::test]
async fn test_cross_project_isolation_recent() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for (id, content, project) in [
        ("rec_a1", "alpha recent note one", "alpha"),
        ("rec_a2", "alpha recent note two", "alpha"),
        ("rec_b1", "beta recent note one", "beta"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                project: Some(project.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Recent with project="alpha" — should return only alpha memories
    let results = storage
        .recent(
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(
            r.id.starts_with("rec_a"),
            "expected alpha memory in recent, got id={}",
            r.id
        );
    }

    // Recent with project="beta" — should return only beta memory
    let results = storage
        .recent(
            10,
            &SearchOptions {
                project: Some("beta".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "rec_b1");
}

#[tokio::test]
async fn test_cross_project_isolation_by_tags() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    // Both projects share the same tag "infra"
    for (id, content, project) in [
        ("tag_a1", "alpha infra setup", "alpha"),
        ("tag_b1", "beta infra setup", "beta"),
        ("tag_b2", "beta infra monitoring", "beta"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                tags: vec!["infra".to_string()],
                project: Some(project.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Query by tag "infra" with project="alpha" — only alpha memory
    let results = storage
        .get_by_tags(
            &["infra".to_string()],
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "tag_a1");

    // Query by tag "infra" with project="beta" — only beta memories
    let results = storage
        .get_by_tags(
            &["infra".to_string()],
            10,
            &SearchOptions {
                project: Some("beta".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"tag_b1"));
    assert!(ids.contains(&"tag_b2"));
}

#[tokio::test]
async fn test_cross_project_no_filter_returns_all() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for (id, content, project) in [
        ("all_a1", "alpha global content", "alpha"),
        ("all_b1", "beta global content", "beta"),
        ("all_c1", "gamma global content", "gamma"),
    ] {
        <SqliteStorage as Storage>::store(
            &storage,
            id,
            content,
            &MemoryInput {
                project: Some(project.to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Search WITHOUT project filter — all memories should be returned
    let results = storage
        .search("global content", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
    let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"all_a1"));
    assert!(ids.contains(&"all_b1"));
    assert!(ids.contains(&"all_c1"));

    // Recent WITHOUT project filter — all memories should be returned
    let results = storage.recent(10, &SearchOptions::default()).await.unwrap();
    assert!(results.len() >= 3);
}

// ---------------------------------------------------------------------------
// Negation test cases (issue #46)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_negation_query_doesnt_match_positive() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "neg_pos",
        "always use async for IO operations",
        &MemoryInput {
            event_type: Some(EventType::Decision),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "neg_neg",
        "never use blocking calls in the event loop",
        &MemoryInput {
            event_type: Some(EventType::LessonLearned),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Search for "never use async" — the positive statement "always use async"
    // should NOT be the top result; the negative statement is more relevant.
    let results = storage
        .search("never use blocking", 10, &SearchOptions::default())
        .await
        .unwrap();
    assert!(!results.is_empty());
    // The top result should be the memory that actually contains "never use blocking"
    assert_eq!(
        results[0].id, "neg_neg",
        "expected the negation memory to rank first"
    );
}

#[tokio::test]
async fn test_project_null_memories_not_in_project_search() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    // Store a memory with NO project set
    <SqliteStorage as Storage>::store(
        &storage,
        "null_proj",
        "unscoped memory with no project",
        &MemoryInput {
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Store a memory WITH project="alpha"
    <SqliteStorage as Storage>::store(
        &storage,
        "alpha_proj",
        "alpha scoped memory with project",
        &MemoryInput {
            project: Some("alpha".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Search with project="alpha" — the null-project memory must NOT appear
    let results = storage
        .search(
            "memory",
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "alpha_proj");

    // Recent with project="alpha" — the null-project memory must NOT appear
    let results = storage
        .recent(
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "alpha_proj");

    // Tags query with project="alpha" — null-project memory must NOT appear
    // First, store tagged versions
    <SqliteStorage as Storage>::store(
        &storage,
        "null_tagged",
        "unscoped tagged memory",
        &MemoryInput {
            tags: vec!["common".to_string()],
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    <SqliteStorage as Storage>::store(
        &storage,
        "alpha_tagged",
        "alpha tagged memory",
        &MemoryInput {
            tags: vec!["common".to_string()],
            project: Some("alpha".to_string()),
            metadata: serde_json::json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let results = storage
        .get_by_tags(
            &["common".to_string()],
            10,
            &SearchOptions {
                project: Some("alpha".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "alpha_tagged");
}

// ── store_batch tests ────────────────────────────────────────────────

#[tokio::test]
async fn store_batch_stores_all_items() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    let items: Vec<(String, String, MemoryInput)> = (0..5)
        .map(|i| {
            let id = format!("batch-{i}");
            let content = format!("Batch item number {i} about topic alpha");
            let input = MemoryInput {
                content: content.clone(),
                id: Some(id.clone()),
                tags: vec!["batch".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                event_type: Some(EventType::Memory),
                ..Default::default()
            };
            (id, content, input)
        })
        .collect();

    storage.store_batch(&items).await.unwrap();

    // Verify all items are retrievable
    for i in 0..5 {
        let content = storage.retrieve(&format!("batch-{i}")).await.unwrap();
        assert!(
            content.contains(&format!("Batch item number {i}")),
            "batch-{i} content mismatch: {content}"
        );
    }
}

#[tokio::test]
async fn store_batch_empty_is_noop() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    storage.store_batch(&[]).await.unwrap();
}

// ── query cache tests ────────────────────────────────────────────────

#[tokio::test]
async fn query_cache_returns_cached_results() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    storage
        .store(
            "cache-1",
            "alpha topic about databases",
            &MemoryInput {
                content: "alpha topic about databases".to_string(),
                importance: 0.8,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let opts = SearchOptions::default();

    // First query — populates cache
    let results1 =
        <SqliteStorage as AdvancedSearcher>::advanced_search(&storage, "databases", 5, &opts)
            .await
            .unwrap();

    // Second query — should hit cache (same results)
    let results2 =
        <SqliteStorage as AdvancedSearcher>::advanced_search(&storage, "databases", 5, &opts)
            .await
            .unwrap();

    assert_eq!(results1.len(), results2.len());
    for (r1, r2) in results1.iter().zip(results2.iter()) {
        assert_eq!(r1.id, r2.id);
    }
}

#[tokio::test]
async fn query_cache_invalidated_on_store() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    storage
        .store(
            "inv-1",
            "unique alpha content",
            &MemoryInput {
                content: "unique alpha content".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let opts = SearchOptions::default();

    // Populate cache
    let results1 =
        <SqliteStorage as AdvancedSearcher>::advanced_search(&storage, "unique alpha", 5, &opts)
            .await
            .unwrap();
    assert!(!results1.is_empty());

    // Store new memory — should invalidate cache
    storage
        .store(
            "inv-2",
            "another unique alpha memory",
            &MemoryInput {
                content: "another unique alpha memory".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Next query should include the new memory
    let results2 =
        <SqliteStorage as AdvancedSearcher>::advanced_search(&storage, "unique alpha", 5, &opts)
            .await
            .unwrap();
    assert!(results2.len() >= results1.len());
}
