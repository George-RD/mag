use std::sync::Arc;
use std::time::Duration;

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::{MemoryInput, SearchOptions, Searcher, Storage};

/// Helper: store a single memory with unique content.
async fn store_one(storage: &SqliteStorage, idx: usize) {
    let id = format!("stress-{idx}");
    let content = format!("Stress test memory number {idx} with unique payload data");
    let input = MemoryInput {
        tags: vec!["stress".to_string()],
        importance: 0.5,
        metadata: serde_json::json!({}),
        ..Default::default()
    };
    <SqliteStorage as Storage>::store(storage, &id, &content, &input)
        .await
        .unwrap();
}

/// Store 10,000 memories, then search. Asserts no panics or errors.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn bulk_store_and_search_performance() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for i in 0..10_000 {
        store_one(&storage, i).await;
    }

    let results = storage
        .search("unique payload data", 20, &SearchOptions::default())
        .await
        .unwrap();

    assert!(
        !results.is_empty(),
        "search should return results after bulk insert"
    );
}

/// Populate 1,000 memories, then spawn 100 concurrent search tasks.
/// All must complete without errors.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_searches() {
    let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());

    for i in 0..1_000 {
        store_one(&storage, i).await;
    }

    let mut handles = Vec::with_capacity(100);
    for task_id in 0..100 {
        let s = Arc::clone(&storage);
        handles.push(tokio::spawn(async move {
            let query = format!("memory number {task_id}");
            let results = s
                .search(&query, 5, &SearchOptions::default())
                .await
                .expect("concurrent search must not fail");
            assert!(
                !results.is_empty(),
                "task {task_id}: search should return results"
            );
        }));
    }

    for handle in handles {
        handle.await.expect("search task must not panic");
    }
}

/// Store 1,000 memories (some with short TTL), then run sweep_expired 50
/// times. Asserts no panics or errors.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn repeated_sweep_no_panic() {
    let storage = SqliteStorage::new_in_memory().unwrap();

    for i in 0..1_000 {
        let id = format!("sweep-{i}");
        let content = format!("Sweepable memory {i}");
        let input = MemoryInput {
            tags: vec!["sweep".to_string()],
            importance: 0.5,
            metadata: serde_json::json!({}),
            // Every 5th memory gets a 1-second TTL so it expires quickly.
            ttl_seconds: if i % 5 == 0 { Some(1) } else { None },
            ..Default::default()
        };
        <SqliteStorage as Storage>::store(&storage, &id, &content, &input)
            .await
            .unwrap();
    }

    // Brief pause to let short-TTL memories expire.
    tokio::time::sleep(Duration::from_secs(2)).await;

    for _ in 0..50 {
        let swept = storage.sweep_expired().await.unwrap();
        // After the first sweep clears expired entries, subsequent sweeps
        // may return 0 — that is fine.
        assert!(swept <= 1_000, "swept count should be reasonable");
    }
}

/// Spawn concurrent writer and reader tasks. Run for up to 5 seconds.
/// Asserts all tasks complete without deadlock.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_read_write() {
    let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());

    // Seed a few memories so readers have something to find.
    for i in 0..100 {
        store_one(&storage, i).await;
    }

    let writer_storage = Arc::clone(&storage);
    let reader_storage = Arc::clone(&storage);

    let writers = tokio::spawn(async move {
        for i in 100..1_100 {
            let id = format!("rw-{i}");
            let content = format!("Concurrent write memory {i}");
            let input = MemoryInput {
                tags: vec!["rw".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            };
            <SqliteStorage as Storage>::store(&*writer_storage, &id, &content, &input)
                .await
                .unwrap();
        }
    });

    let readers = tokio::spawn(async move {
        for i in 0..200 {
            let query = format!("memory {i}");
            let _ = reader_storage
                .search(&query, 5, &SearchOptions::default())
                .await
                .expect("concurrent read must not fail");
        }
    });

    let result = tokio::time::timeout(Duration::from_secs(30), async {
        writers.await.expect("writers must not panic");
        readers.await.expect("readers must not panic");
    })
    .await;

    assert!(
        result.is_ok(),
        "concurrent read/write must complete within timeout (no deadlock)"
    );
}
