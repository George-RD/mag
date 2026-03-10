use romega_memory::memory_core::storage::sqlite::SqliteStorage;
use romega_memory::memory_core::{
    AdvancedSearcher, EventType, MemoryInput, SearchOptions, Storage,
};

async fn seed(
    storage: &SqliteStorage,
    id: &str,
    content: &str,
    event_type: EventType,
    priority: i32,
    tags: &[&str],
) {
    let input = MemoryInput {
        content: content.to_string(),
        event_type: Some(event_type),
        priority: Some(priority),
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        ..Default::default()
    };
    <SqliteStorage as Storage>::store(storage, id, content, &input)
        .await
        .unwrap();
}

#[tokio::test]
async fn multi_session_vector_query_prefers_sqlite_vec_memory() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    seed(
        &storage,
        "ms-decision",
        "Decided to use SQLite for the OMEGA backend because it's zero-config and embedded.",
        EventType::Decision,
        4,
        &["sqlite", "omega"],
    )
    .await;
    seed(
        &storage,
        "ms-vector",
        "Added sqlite-vec extension for vector similarity search in OMEGA.",
        EventType::TaskCompletion,
        3,
        &["sqlite", "omega"],
    )
    .await;
    seed(
        &storage,
        "ms-text",
        "FTS5 full-text search index added to OMEGA for fast keyword queries.",
        EventType::TaskCompletion,
        3,
        &["sqlite", "omega"],
    )
    .await;

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "OMEGA vector search implementation",
        3,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    assert!(!results.is_empty());
    assert!(results[0].content.contains("sqlite-vec extension"));
}

#[tokio::test]
async fn multi_session_ci_cd_query_prefers_migration_step_memory() {
    let storage = SqliteStorage::new_in_memory().unwrap();
    seed(
        &storage,
        "ms-platform",
        "The CI/CD pipeline uses GitHub Actions with separate staging and production workflows.",
        EventType::Decision,
        4,
        &["git"],
    )
    .await;
    seed(
        &storage,
        "ms-migration",
        "Added automated database migration step to the CI/CD pipeline before deployment.",
        EventType::TaskCompletion,
        3,
        &["git"],
    )
    .await;
    seed(
        &storage,
        "ms-schema",
        "OMEGA schema migration system uses ALTER TABLE for backwards-compatible upgrades.",
        EventType::Decision,
        4,
        &["sqlite", "omega"],
    )
    .await;

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &storage,
        "database migration in CI/CD",
        3,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    assert!(!results.is_empty());
    assert!(results[0].content.contains("migration step"));
}
