//! Shared conformance test suite for storage backends.
//!
//! Runs identical assertions against both `SqliteStorage` (in-memory mode)
//! and `MemoryStorage`, catching behavioral divergence early.
//!
//! # Design
//!
//! Generic async helper functions accept `&dyn Trait` references to
//! exercise the shared trait surface. A `conformance_tests!` macro generates
//! a module per backend that calls these helpers with the concrete backend.
//!
//! # Known backend differences
//!
//! - `AdvancedSearcher` and `PhraseSearcher` are unimplemented in
//!   `MemoryStorage` (they require SQLite FTS5). Those tests are skipped
//!   for the memory backend.
//! - `SqliteStorage::import_all` returns `Result<(usize, usize)>`
//!   whereas `MemoryStorage::import_all` returns `Result<usize>`.
//!   The export/import round-trip is tested per-backend outside the macro.
//! - `SqliteStorage::stats()` is a concrete method (not a trait), so it is
//!   tested only on the SQLite backend.

use std::path::PathBuf;
use std::sync::Arc;

use mag::memory_core::scoring_strategy::DefaultScoringStrategy;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::storage::memory::MemoryStorage;
use mag::memory_core::{
    Deleter, Lister, MemoryInput, MemoryUpdate, PlaceholderEmbedder, Retriever, SearchOptions,
    Searcher, SemanticSearcher, StatsProvider, Storage, Tagger, Updater,
};

// ── Helpers ──────────────────────────────────────────────────────────────

fn make_input(content: &str) -> MemoryInput {
    MemoryInput {
        content: content.to_string(),
        importance: 0.5,
        metadata: serde_json::json!({}),
        ..Default::default()
    }
}

fn make_input_with_tags(content: &str, tags: &[&str]) -> MemoryInput {
    MemoryInput {
        content: content.to_string(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        importance: 0.5,
        metadata: serde_json::json!({}),
        ..Default::default()
    }
}

fn make_input_with_session(content: &str, session_id: &str) -> MemoryInput {
    MemoryInput {
        content: content.to_string(),
        importance: 0.5,
        metadata: serde_json::json!({}),
        session_id: Some(session_id.to_string()),
        ..Default::default()
    }
}

// ── Backend constructors ─────────────────────────────────────────────────

fn make_sqlite_storage() -> SqliteStorage {
    SqliteStorage::new_with_path(PathBuf::from(":memory:"), Arc::new(PlaceholderEmbedder)).unwrap()
}

fn make_memory_storage() -> MemoryStorage {
    MemoryStorage::new(
        Arc::new(PlaceholderEmbedder),
        Arc::new(DefaultScoringStrategy::new()),
    )
}

// ── Generic conformance test functions ───────────────────────────────────
//
// Each function takes trait-object references so it can be called with any
// backend. The backend must implement all tested traits. Functions are
// grouped by the trait they exercise.

/// A bundle of trait-object references for a single backend instance.
/// Avoids repeating N trait-object arguments per test function.
struct Backend<'a> {
    storage: &'a (dyn Storage + Send + Sync),
    retriever: &'a (dyn Retriever + Send + Sync),
    updater: &'a (dyn Updater + Send + Sync),
    deleter: &'a (dyn Deleter + Send + Sync),
    tagger: &'a (dyn Tagger + Send + Sync),
    lister: &'a (dyn Lister + Send + Sync),
    searcher: &'a (dyn Searcher + Send + Sync),
    semantic_searcher: &'a (dyn SemanticSearcher + Send + Sync),
    stats_provider: &'a (dyn StatsProvider + Send + Sync),
}

// ── Store + Retrieve ─────────────────────────────────────────────────────

async fn test_store_retrieve_round_trip(b: &Backend<'_>) {
    let input = make_input("hello world");
    b.storage
        .store("rt-1", "hello world", &input)
        .await
        .unwrap();
    let content = b.retriever.retrieve("rt-1").await.unwrap();
    assert_eq!(content, "hello world");
}

async fn test_retrieve_nonexistent(b: &Backend<'_>) {
    let result = b.retriever.retrieve("no-such-id").await;
    assert!(result.is_err());
}

// ── Update content ───────────────────────────────────────────────────────

async fn test_update_content_persists(b: &Backend<'_>) {
    let input = make_input("original");
    b.storage.store("upd-1", "original", &input).await.unwrap();

    let update = MemoryUpdate {
        content: Some("updated".to_string()),
        ..Default::default()
    };
    b.updater.update("upd-1", &update).await.unwrap();

    let content = b.retriever.retrieve("upd-1").await.unwrap();
    assert_eq!(content, "updated");
}

// ── Update tags ──────────────────────────────────────────────────────────

async fn test_update_tags_persists(b: &Backend<'_>) {
    let input = make_input_with_tags("tagged", &["alpha", "beta"]);
    b.storage
        .store("tag-upd-1", "tagged", &input)
        .await
        .unwrap();

    // Verify original tags.
    let found = b
        .tagger
        .get_by_tags(&["alpha".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(found.len(), 1);

    // Update tags.
    let update = MemoryUpdate {
        tags: Some(vec!["gamma".to_string(), "delta".to_string()]),
        ..Default::default()
    };
    b.updater.update("tag-upd-1", &update).await.unwrap();

    // Old tag no longer matches.
    let found_old = b
        .tagger
        .get_by_tags(&["alpha".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert!(found_old.is_empty());

    // New tags match.
    let found_new = b
        .tagger
        .get_by_tags(&["gamma".to_string()], 10, &SearchOptions::default())
        .await
        .unwrap();
    assert_eq!(found_new.len(), 1);
    assert_eq!(found_new[0].id, "tag-upd-1");
}

// ── Delete ───────────────────────────────────────────────────────────────

async fn test_delete_removes_memory(b: &Backend<'_>) {
    let input = make_input("ephemeral");
    b.storage.store("del-1", "ephemeral", &input).await.unwrap();

    let deleted = b.deleter.delete("del-1").await.unwrap();
    assert!(deleted);

    let result = b.retriever.retrieve("del-1").await;
    assert!(result.is_err());
}

async fn test_delete_nonexistent(b: &Backend<'_>) {
    let deleted = b.deleter.delete("ghost").await.unwrap();
    assert!(!deleted);
}

// ── List with pagination ─────────────────────────────────────────────────

async fn test_list_pagination(b: &Backend<'_>) {
    for i in 0..5 {
        let id = format!("pg-{i}");
        let content = format!("item {i}");
        let input = make_input(&content);
        b.storage.store(&id, &content, &input).await.unwrap();
    }

    let opts = SearchOptions::default();

    // Full list.
    let all = b.lister.list(0, 100, &opts).await.unwrap();
    assert_eq!(all.total, 5);
    assert_eq!(all.memories.len(), 5);

    // Page: skip 2, take 2.
    let page = b.lister.list(2, 2, &opts).await.unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(page.memories.len(), 2);

    // Page past the end.
    let empty = b.lister.list(10, 5, &opts).await.unwrap();
    assert_eq!(empty.total, 5);
    assert!(empty.memories.is_empty());
}

// ── Tag search (AND semantics) ───────────────────────────────────────────

async fn test_tag_search_and_semantics(b: &Backend<'_>) {
    let input_ab = make_input_with_tags("both tags", &["rust", "async"]);
    b.storage
        .store("ts-1", "both tags", &input_ab)
        .await
        .unwrap();

    let input_a = make_input_with_tags("one tag", &["rust"]);
    b.storage.store("ts-2", "one tag", &input_a).await.unwrap();

    let opts = SearchOptions::default();

    // Single tag matches both.
    let one = b
        .tagger
        .get_by_tags(&["rust".to_string()], 10, &opts)
        .await
        .unwrap();
    assert_eq!(one.len(), 2);

    // Both tags (AND) matches only ts-1.
    let both = b
        .tagger
        .get_by_tags(&["rust".to_string(), "async".to_string()], 10, &opts)
        .await
        .unwrap();
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].id, "ts-1");
}

// ── Text search ──────────────────────────────────────────────────────────

async fn test_text_search_matches_content(b: &Backend<'_>) {
    let input_match = make_input("the quick brown fox");
    b.storage
        .store("txt-1", "the quick brown fox", &input_match)
        .await
        .unwrap();

    let input_nomatch = make_input("lazy dog sleeps");
    b.storage
        .store("txt-2", "lazy dog sleeps", &input_nomatch)
        .await
        .unwrap();

    let results = b
        .searcher
        .search("quick brown", 10, &SearchOptions::default())
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "txt-1");
}

// ── StatsProvider ────────────────────────────────────────────────────────

async fn test_stats_type_counts(b: &Backend<'_>) {
    let input = make_input("stat item");
    b.storage.store("st-1", "stat item", &input).await.unwrap();
    b.storage
        .store(
            "st-2",
            "another stat item",
            &make_input("another stat item"),
        )
        .await
        .unwrap();

    let stats = b.stats_provider.type_stats().await.unwrap();
    let total = stats["_total"].as_i64().unwrap();
    assert_eq!(total, 2);
}

async fn test_stats_session_counts(b: &Backend<'_>) {
    let input1 = make_input_with_session("session memory 1", "sess-A");
    b.storage
        .store("ss-1", "session memory 1", &input1)
        .await
        .unwrap();

    let input2 = make_input_with_session("session memory 2", "sess-A");
    b.storage
        .store("ss-2", "session memory 2", &input2)
        .await
        .unwrap();

    let input3 = make_input_with_session("session memory 3", "sess-B");
    b.storage
        .store("ss-3", "session memory 3", &input3)
        .await
        .unwrap();

    let stats = b.stats_provider.session_stats().await.unwrap();
    let sessions = stats["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());

    let total = stats["total_sessions"].as_i64().unwrap();
    assert_eq!(total, 2);
}

async fn test_stats_access_rates(b: &Backend<'_>) {
    let input = make_input("access test");
    b.storage
        .store("ar-1", "access test", &input)
        .await
        .unwrap();

    // Access once to bump the counter.
    let _ = b.retriever.retrieve("ar-1").await.unwrap();

    let stats = b.stats_provider.access_rate_stats().await.unwrap();
    let total = stats["total_memories"].as_i64().unwrap();
    assert_eq!(total, 1);
}

// ── Semantic search ──────────────────────────────────────────────────────

async fn test_semantic_search_returns_results(b: &Backend<'_>) {
    let input = make_input("machine learning transformers");
    b.storage
        .store("sem-1", "machine learning transformers", &input)
        .await
        .unwrap();

    let input2 = make_input("cooking recipes for pasta");
    b.storage
        .store("sem-2", "cooking recipes for pasta", &input2)
        .await
        .unwrap();

    let results = b
        .semantic_searcher
        .semantic_search(
            "deep learning neural networks",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();

    // Both should be returned (PlaceholderEmbedder uses SHA256 hash,
    // so scores are deterministic but not semantically meaningful).
    assert!(!results.is_empty());

    // All results should have a finite score.
    for r in &results {
        assert!(r.score.is_finite());
    }
}

// ── Update error cases ───────────────────────────────────────────────────

async fn test_update_empty_returns_error(b: &Backend<'_>) {
    let input = make_input("will not update");
    b.storage
        .store("err-1", "will not update", &input)
        .await
        .unwrap();

    let empty_update = MemoryUpdate::default();
    let result = b.updater.update("err-1", &empty_update).await;
    assert!(result.is_err());
}

async fn test_update_nonexistent_returns_error(b: &Backend<'_>) {
    let update = MemoryUpdate {
        content: Some("nope".to_string()),
        ..Default::default()
    };
    let result = b.updater.update("missing", &update).await;
    assert!(result.is_err());
}

// ── Conformance macro ────────────────────────────────────────────────────
//
// Generates one `#[tokio::test]` per conformance check, each creating a
// fresh backend instance. This isolates tests from each other.

macro_rules! conformance_tests {
    ($mod_name:ident, $make_storage:expr) => {
        mod $mod_name {
            use super::*;

            fn make_backend_ref(
                s: &(
                     impl Storage
                     + Retriever
                     + Updater
                     + Deleter
                     + Tagger
                     + Lister
                     + Searcher
                     + SemanticSearcher
                     + StatsProvider
                 ),
            ) -> Backend<'_> {
                Backend {
                    storage: s,
                    retriever: s,
                    updater: s,
                    deleter: s,
                    tagger: s,
                    lister: s,
                    searcher: s,
                    semantic_searcher: s,
                    stats_provider: s,
                }
            }

            #[tokio::test]
            async fn store_retrieve_round_trip() {
                let s = $make_storage;
                test_store_retrieve_round_trip(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn retrieve_nonexistent_returns_error() {
                let s = $make_storage;
                test_retrieve_nonexistent(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn update_content_persists() {
                let s = $make_storage;
                test_update_content_persists(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn update_tags_persists() {
                let s = $make_storage;
                test_update_tags_persists(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn delete_removes_memory() {
                let s = $make_storage;
                test_delete_removes_memory(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn delete_nonexistent_returns_false() {
                let s = $make_storage;
                test_delete_nonexistent(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn list_pagination() {
                let s = $make_storage;
                test_list_pagination(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn tag_search_and_semantics() {
                let s = $make_storage;
                test_tag_search_and_semantics(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn text_search_matches_content() {
                let s = $make_storage;
                test_text_search_matches_content(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn stats_type_counts() {
                let s = $make_storage;
                test_stats_type_counts(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn stats_session_counts() {
                let s = $make_storage;
                test_stats_session_counts(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn stats_access_rates() {
                let s = $make_storage;
                test_stats_access_rates(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn semantic_search_returns_results() {
                let s = $make_storage;
                test_semantic_search_returns_results(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn update_empty_returns_error() {
                let s = $make_storage;
                test_update_empty_returns_error(&make_backend_ref(&s)).await;
            }

            #[tokio::test]
            async fn update_nonexistent_returns_error() {
                let s = $make_storage;
                test_update_nonexistent_returns_error(&make_backend_ref(&s)).await;
            }
        }
    };
}

// ── Generate conformance modules ─────────────────────────────────────────

conformance_tests!(sqlite_conformance, make_sqlite_storage());
conformance_tests!(memory_conformance, make_memory_storage());

// ── Export / Import round-trip ────────────────────────────────────────────
//
// Export/import APIs differ between backends:
// - `SqliteStorage::import_all` returns `Result<(usize, usize)>` (memories, relationships)
// - `MemoryStorage::import_all` returns `Result<usize>` (memories only)
//
// These are tested per-backend outside the macro.

mod sqlite_export_import {
    use super::*;

    #[tokio::test]
    async fn export_import_round_trip() {
        let storage = make_sqlite_storage();

        // Store two memories with tags.
        let input1 = make_input_with_tags("first memory", &["tag-a"]);
        Storage::store(&storage, "exp-1", "first memory", &input1)
            .await
            .unwrap();

        let input2 = make_input_with_tags("second memory", &["tag-b", "tag-c"]);
        Storage::store(&storage, "exp-2", "second memory", &input2)
            .await
            .unwrap();

        // Export.
        let exported = storage.export_all().await.unwrap();

        // Verify JSON is valid and contains memories.
        let parsed: serde_json::Value = serde_json::from_str(&exported).unwrap();
        let memories = parsed["memories"].as_array().unwrap();
        assert_eq!(memories.len(), 2);

        // Import into a fresh instance.
        let fresh = make_sqlite_storage();
        let (mem_count, _rel_count) = fresh.import_all(&exported).await.unwrap();
        assert_eq!(mem_count, 2);

        // Verify data preserved.
        let content1 = Retriever::retrieve(&fresh, "exp-1").await.unwrap();
        assert_eq!(content1, "first memory");

        let content2 = Retriever::retrieve(&fresh, "exp-2").await.unwrap();
        assert_eq!(content2, "second memory");

        // Verify tags survived via tag search.
        let tagged = Tagger::get_by_tags(
            &fresh,
            &["tag-b".to_string()],
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "exp-2");
    }
}

mod memory_export_import {
    use super::*;

    #[tokio::test]
    async fn export_import_round_trip() {
        let storage = make_memory_storage();

        // Store two memories with tags.
        let input1 = make_input_with_tags("first memory", &["tag-a"]);
        Storage::store(&storage, "exp-1", "first memory", &input1)
            .await
            .unwrap();

        let input2 = make_input_with_tags("second memory", &["tag-b", "tag-c"]);
        Storage::store(&storage, "exp-2", "second memory", &input2)
            .await
            .unwrap();

        // Export.
        let exported = storage.export_all().await.unwrap();

        // Verify JSON is valid and contains memories.
        let parsed: serde_json::Value = serde_json::from_str(&exported).unwrap();
        let memories = parsed["memories"].as_array().unwrap();
        assert_eq!(memories.len(), 2);

        // Import into a fresh instance.
        let fresh = make_memory_storage();
        let mem_count = fresh.import_all(&exported).await.unwrap();
        assert_eq!(mem_count, 2);

        // Verify data preserved.
        let content1 = Retriever::retrieve(&fresh, "exp-1").await.unwrap();
        assert_eq!(content1, "first memory");

        let content2 = Retriever::retrieve(&fresh, "exp-2").await.unwrap();
        assert_eq!(content2, "second memory");

        // Verify tags survived via tag search.
        let tagged = Tagger::get_by_tags(
            &fresh,
            &["tag-b".to_string()],
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "exp-2");
    }
}
