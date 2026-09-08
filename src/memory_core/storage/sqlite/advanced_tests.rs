use super::pipeline::{advanced_fts_candidate_limit, collect_fts_candidates};
use crate::memory_core::{MemoryInput, SearchOptions, Storage, storage::SqliteStorage};
use rusqlite::params;

#[tokio::test]
async fn generation_rejection_clears_query_and_hot_caches() -> anyhow::Result<()> {
    use crate::memory_core::AdvancedSearcher;
    let storage = SqliteStorage::new_in_memory()?;
    storage
        .store("alpha", "alpha memory", &MemoryInput::default())
        .await?;
    storage
        .advanced_search("alpha", 5, &SearchOptions::default())
        .await?;
    assert!(!storage.query_cache.lock().unwrap().is_empty());
    assert!(storage.hot_cache.as_ref().unwrap().is_initialized());
    {
        let conn = storage.pool.writer()?;
        super::super::schema::advance_embedding_generation(&conn)?;
    }
    let error = storage
        .advanced_search("alpha", 5, &SearchOptions::default())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("embedding generation mismatch"));
    assert!(storage.query_cache.lock().unwrap().is_empty());
    assert!(!storage.hot_cache.as_ref().unwrap().is_initialized());
    Ok(())
}

#[test]
fn advanced_fts_candidate_limit_is_bounded() {
    assert_eq!(advanced_fts_candidate_limit(1), 100);
    assert_eq!(advanced_fts_candidate_limit(10), 200);
    assert_eq!(advanced_fts_candidate_limit(1_000), 5_000);
    assert_eq!(advanced_fts_candidate_limit(5_001), 5_001);
}

#[tokio::test]
async fn bounded_fts_candidates_preserve_created_at_filters() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for idx in 0..(super::pipeline::ADVANCED_FTS_CANDIDATE_MIN + 20) {
        let id = format!("old-{idx}");
        let content = format!("alpha {idx}");
        <SqliteStorage as Storage>::store(
            &storage,
            &id,
            &content,
            &MemoryInput {
                content: content.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    <SqliteStorage as Storage>::store(
        &storage,
        "recent-match",
        "alpha context details",
        &MemoryInput {
            content: "alpha context details".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count,
        (super::pipeline::ADVANCED_FTS_CANDIDATE_MIN + 21) as i64,
        "FTS bound fixture must exceed the candidate limit after canonical dedup"
    );
    conn.execute(
        "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id LIKE 'old-%'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
        params![],
    )
    .unwrap();

    assert_unfiltered_limit_excludes(&conn, &storage, "recent-match");
    let candidates = collect_fts_candidates(
        &conn,
        "alpha",
        1,
        &SearchOptions {
            created_after: Some("2025-01-01T00:00:00.000Z".to_string()),
            ..Default::default()
        },
        true,
        &storage.scoring_params,
    )
    .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, "recent-match");
}

#[tokio::test]
async fn bounded_fts_candidates_preserve_event_at_filters() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for idx in 0..(super::pipeline::ADVANCED_FTS_CANDIDATE_MIN + 20) {
        let id = format!("old-event-{idx}");
        let content = format!("alpha {idx}");
        <SqliteStorage as Storage>::store(
            &storage,
            &id,
            &content,
            &MemoryInput {
                content: content.clone(),
                referenced_date: Some("2000-01-01T00:00:00.000Z".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    <SqliteStorage as Storage>::store(
        &storage,
        "recent-event-match",
        "alpha context details",
        &MemoryInput {
            content: "alpha context details".to_string(),
            referenced_date: Some("2025-06-01T00:00:00.000Z".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let conn = storage.test_conn().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count,
        (super::pipeline::ADVANCED_FTS_CANDIDATE_MIN + 21) as i64,
        "FTS bound fixture must exceed the candidate limit after canonical dedup"
    );
    assert_unfiltered_limit_excludes(&conn, &storage, "recent-event-match");
    let recent_candidates = collect_fts_candidates(
        &conn,
        "alpha",
        1,
        &SearchOptions {
            event_after: Some("2025-01-01T00:00:00.000Z".to_string()),
            ..Default::default()
        },
        true,
        &storage.scoring_params,
    )
    .unwrap();

    assert_eq!(recent_candidates.len(), 1);
    assert_eq!(recent_candidates[0].0, "recent-event-match");
}

/// Integration test: keyword-intent queries go through KeywordOnlyStrategy
/// dispatch and still return relevant FTS5 results.
#[tokio::test]
async fn keyword_dispatch_returns_fts_results() {
    use crate::memory_core::AdvancedSearcher;

    let (storage, model) = routing_storage();

    // Store memories with identifiable content.
    <SqliteStorage as Storage>::store(
        &storage,
        "func-1",
        "SqliteStorage implementation details",
        &MemoryInput {
            content: "SqliteStorage implementation details".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    <SqliteStorage as Storage>::store(
        &storage,
        "func-2",
        "McpMemoryServer handles tool routing",
        &MemoryInput {
            content: "McpMemoryServer handles tool routing".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // CamelCase query triggers keyword intent -> KeywordOnlyStrategy.
    let results = storage
        .advanced_search("SqliteStorage", 10, &SearchOptions::default())
        .await
        .unwrap();

    assert_eq!(
        model.0.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "keyword dispatch must skip query embeddings"
    );
    assert!(!results.is_empty(), "keyword query should return results");
    assert!(
        results.iter().any(|r| r.content.contains("SqliteStorage")),
        "should find the SqliteStorage memory"
    );
}

/// Integration test: non-keyword queries still go through the full pipeline.
#[tokio::test]
async fn non_keyword_query_uses_full_pipeline() {
    use crate::memory_core::AdvancedSearcher;

    let (storage, model) = routing_storage();

    <SqliteStorage as Storage>::store(
        &storage,
        "mem-1",
        "The database uses SQLite for storage",
        &MemoryInput {
            content: "The database uses SQLite for storage".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Natural language query -> NOT keyword intent -> full pipeline.
    let results = storage
        .advanced_search(
            "What database does the project use?",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();

    assert!(
        model.0.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "natural-language dispatch must compute query embeddings"
    );
    // Should still return results through the full pipeline.
    assert!(!results.is_empty(), "full pipeline should return results");
}

/// Empty and whitespace-only queries must take the FTS-only dispatch
/// path; running vector search with an empty embedding produces no
/// useful signal. The call must return cleanly (no panic from a
/// zero-length embedding hitting downstream cosine math) and yield
/// an empty result set, since FTS5 with a blank query matches
/// nothing.
#[tokio::test]
async fn blank_query_routes_to_fts_only() {
    use crate::memory_core::AdvancedSearcher;

    let (storage, model) = routing_storage();

    <SqliteStorage as Storage>::store(
        &storage,
        "blank-1",
        "alpha entry one",
        &MemoryInput {
            content: "alpha entry one".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    for query in ["", "   ", "\t\n  "] {
        let results = storage
            .advanced_search(query, 5, &SearchOptions::default())
            .await
            .expect("blank query must dispatch cleanly");
        assert_eq!(
            model.0.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "blank dispatch must skip query embeddings"
        );
        // FTS5 with an empty query matches nothing, so we expect an empty
        // result set rather than a panic or an embedding-driven scan.
        assert!(
            results.is_empty(),
            "blank query {query:?} should yield no FTS5 matches"
        );
    }
}

fn assert_unfiltered_limit_excludes(
    conn: &rusqlite::Connection,
    storage: &SqliteStorage,
    excluded_id: &str,
) {
    let unfiltered = collect_fts_candidates(
        conn,
        "alpha",
        1,
        &SearchOptions::default(),
        true,
        &storage.scoring_params,
    )
    .unwrap();
    assert_eq!(
        unfiltered.len(),
        super::pipeline::ADVANCED_FTS_CANDIDATE_MIN
    );
    assert!(
        !unfiltered.iter().any(|(id, _, _)| id == excluded_id),
        "fixture must put the filtered match outside the unfiltered candidate limit"
    );
}

#[derive(Debug, Default)]
struct QueryCountingModel(std::sync::atomic::AtomicUsize);

impl crate::memory_core::EmbeddingModel for QueryCountingModel {
    fn dimension(&self) -> usize {
        4
    }
    fn embedding_space_identity(&self) -> &str {
        "test-query-routing"
    }
    fn embed_for(
        &self,
        kind: crate::memory_core::EmbeddingInputKind,
        _text: &str,
    ) -> anyhow::Result<Vec<f32>> {
        if kind == crate::memory_core::EmbeddingInputKind::Query {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(vec![1.0, 0.0, 0.0, 0.0])
    }
}

fn routing_storage() -> (SqliteStorage, std::sync::Arc<QueryCountingModel>) {
    let model = std::sync::Arc::new(QueryCountingModel::default());
    let storage = SqliteStorage::new_in_memory_with_embedding_model(model.clone()).unwrap();
    (storage, model)
}
