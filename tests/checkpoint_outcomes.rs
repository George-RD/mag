use std::path::PathBuf;
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{CheckpointInput, PlaceholderEmbedder};

async fn runtime_at(path: PathBuf) -> LocalMemoryRuntime {
    let storage = tokio::task::spawn_blocking(move || {
        SqliteStorage::new_with_path(path, Arc::new(PlaceholderEmbedder))
    })
    .await
    .expect("SQLite initialization task should not panic")
    .expect("SQLite storage should initialize");
    LocalMemoryRuntime::from_storage(storage)
}

fn checkpoint(progress: &str) -> CheckpointInput {
    CheckpointInput {
        task_title: "atomic checkpoint task".to_string(),
        progress: progress.to_string(),
        plan: Some("preserve checkpoint continuity".to_string()),
        files_touched: None,
        decisions: None,
        key_context: None,
        next_steps: Some("continue from the persisted number".to_string()),
        session_id: Some("atomic-checkpoint-session".to_string()),
        project: Some("atomic-checkpoint-project".to_string()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checkpoint_saves_allocate_unique_numbers_across_pools() {
    let temp = tempfile::tempdir().expect("temporary directory should initialize");
    let path = temp.path().join("memory.db");
    let first = runtime_at(path.clone()).await;
    let second = runtime_at(path).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let first_save = {
        let barrier = Arc::clone(&barrier);
        async move {
            barrier.wait().await;
            first
                .save_checkpoint_outcome(checkpoint("first concurrent writer"))
                .await
        }
    };
    let second_save = {
        let barrier = Arc::clone(&barrier);
        async move {
            barrier.wait().await;
            second
                .save_checkpoint_outcome(checkpoint("second concurrent writer"))
                .await
        }
    };

    let (first_outcome, second_outcome) = tokio::join!(first_save, second_save);
    let first_outcome = first_outcome.expect("first checkpoint should save");
    let second_outcome = second_outcome.expect("second checkpoint should save");

    let mut numbers = vec![
        first_outcome.checkpoint_number,
        second_outcome.checkpoint_number,
    ];
    numbers.sort_unstable();
    assert_eq!(numbers, vec![1, 2]);
    assert_ne!(first_outcome.memory_id, second_outcome.memory_id);

    let verifier = runtime_at(temp.path().join("memory.db")).await;
    let checkpoints = verifier
        .resume_task(
            "atomic checkpoint task",
            Some("atomic-checkpoint-project"),
            10,
        )
        .await
        .expect("saved checkpoints should be resumable");
    let mut persisted_numbers: Vec<i64> = checkpoints
        .iter()
        .map(|entry| {
            entry["metadata"]["checkpoint_number"]
                .as_i64()
                .expect("checkpoint metadata should contain its number")
        })
        .collect();
    persisted_numbers.sort_unstable();
    assert_eq!(persisted_numbers, vec![1, 2]);
}

#[tokio::test]
async fn deduplicated_checkpoint_returns_the_existing_persisted_outcome() {
    let temp = tempfile::tempdir().expect("temporary directory should initialize");
    let runtime = runtime_at(temp.path().join("memory.db")).await;
    let input = checkpoint("identical progress should retain canonical deduplication");

    let first = runtime
        .save_checkpoint_outcome(input.clone())
        .await
        .expect("first checkpoint should save");
    let repeated = runtime
        .save_checkpoint_outcome(input)
        .await
        .expect("repeated checkpoint should resolve its persisted outcome");

    assert_eq!(repeated, first);
    let checkpoints = runtime
        .resume_task(
            "atomic checkpoint task",
            Some("atomic-checkpoint-project"),
            10,
        )
        .await
        .expect("checkpoint should remain resumable");
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0]["metadata"]["checkpoint_number"], 1);
}
