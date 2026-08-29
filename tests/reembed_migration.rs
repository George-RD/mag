use std::sync::Arc;

use anyhow::{Result, bail};
use mag::LocalMemoryRuntime;
use mag::memory_core::storage::{ReembedOptions, SqliteStorage};
use mag::memory_core::{EmbeddingInputKind, EmbeddingModel, MemoryInput};
use tempfile::tempdir;

#[derive(Debug)]
struct TestEmbeddingModel {
    identity: &'static str,
    dimension: usize,
    fail_on: Option<&'static str>,
}

impl TestEmbeddingModel {
    fn new(identity: &'static str, dimension: usize) -> Self {
        Self {
            identity,
            dimension,
            fail_on: None,
        }
    }

    fn failing_on(identity: &'static str, dimension: usize, text: &'static str) -> Self {
        Self {
            identity,
            dimension,
            fail_on: Some(text),
        }
    }
}

impl EmbeddingModel for TestEmbeddingModel {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embedding_space_identity(&self) -> &str {
        self.identity
    }

    fn embed_for(&self, _input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>> {
        if self.fail_on.is_some_and(|needle| text.contains(needle)) {
            bail!("intentional embedding failure for {text}");
        }

        let mut embedding = vec![0.0; self.dimension];
        if let Some(first) = embedding.first_mut() {
            *first = if text.contains("alpha") { 1.0 } else { 0.5 };
        }
        if self.dimension > 1 {
            embedding[1] = if text.contains("beta") { 1.0 } else { 0.25 };
        }
        Ok(embedding)
    }
}

async fn seed_database(path: &std::path::Path, model: Arc<dyn EmbeddingModel>) -> Result<()> {
    let storage = SqliteStorage::new_with_path_and_embedding_model(path.to_path_buf(), model)?;
    let runtime = LocalMemoryRuntime::from_storage(storage);

    runtime
        .store_raw("alpha", "alpha memory", &MemoryInput::default())
        .await?;
    runtime
        .store_raw("beta", "beta memory", &MemoryInput::default())
        .await?;
    Ok(())
}

#[tokio::test]
async fn reembed_dry_run_reports_affected_memories_without_changing_space() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-b", 4));
    seed_database(&path, Arc::clone(&source)).await?;

    let report = LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: true,
        },
    )
    .await?;

    assert_eq!(report.source_embedding_space, "space-a");
    assert_eq!(report.target_embedding_space, "space-b");
    assert_eq!(report.memory_count, 2);
    assert_eq!(report.migrated_count, 0);
    assert!(report.backup_path.is_none());

    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), target).is_err());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path, source).is_ok());
    Ok(())
}

#[tokio::test]
async fn reembed_migrates_same_dimension_space_atomically_and_creates_backup() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-b", 4));
    seed_database(&path, Arc::clone(&source)).await?;

    let report = LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await?;

    assert_eq!(report.memory_count, 2);
    assert_eq!(report.migrated_count, 2);
    assert!(
        report
            .backup_path
            .as_ref()
            .is_some_and(|path| path.exists())
    );
    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), target).is_ok());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path, source).is_err());
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
#[tokio::test]
async fn reembed_rebuilds_vector_index_for_dimension_change() -> Result<()> {
    use rusqlite::{Connection, params};

    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-b", 6));
    seed_database(&path, Arc::clone(&source)).await?;

    let report = LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await?;

    assert_eq!(report.target_dimension, 6);
    assert_eq!(report.migrated_count, 2);
    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), target).is_ok());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), source).is_err());

    let conn = Connection::open(&path)?;
    let blob_lengths = {
        let mut stmt = conn.prepare("SELECT length(embedding) FROM memories ORDER BY id")?;
        stmt.query_map([], |row| row.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    assert_eq!(blob_lengths, vec![24, 24]);

    let target_probe: Vec<u8> = vec![0.0_f32; 6]
        .into_iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    conn.execute(
        "INSERT INTO vec_memories(memory_id, embedding) VALUES ('__target_probe__', ?1)",
        params![target_probe],
    )?;
    conn.execute(
        "DELETE FROM vec_memories WHERE memory_id = '__target_probe__'",
        [],
    )?;

    let source_probe: Vec<u8> = vec![0.0_f32; 4]
        .into_iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    conn.execute(
        "INSERT INTO vec_memories(memory_id, embedding) VALUES ('__source_probe__', ?1)",
        params![source_probe],
    )
    .expect_err("old-dimension vectors must not fit the rebuilt index");
    Ok(())
}

#[tokio::test]
async fn reembed_failure_rolls_back_vectors_and_embedding_space_identity() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> =
        Arc::new(TestEmbeddingModel::failing_on("space-b", 4, "beta"));
    seed_database(&path, Arc::clone(&source)).await?;

    let result = LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await;

    assert!(result.is_err());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), source).is_ok());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path, target).is_err());
    Ok(())
}

#[cfg(not(feature = "sqlite-vec"))]
#[tokio::test]
async fn reembed_refuses_existing_vector_index_without_sqlite_vec_support() -> Result<()> {
    use rusqlite::Connection;

    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> = Arc::new(TestEmbeddingModel::new("space-b", 4));
    seed_database(&path, Arc::clone(&source)).await?;

    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE vec_memories (memory_id TEXT PRIMARY KEY, embedding BLOB NOT NULL);",
    )?;
    drop(conn);

    let error = LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await
    .expect_err("migration must not leave an existing vector index stale");

    assert!(error.to_string().contains("sqlite-vec"));
    assert!(SqliteStorage::new_with_path_and_embedding_model(path.clone(), source).is_ok());
    assert!(SqliteStorage::new_with_path_and_embedding_model(path, target).is_err());
    Ok(())
}
