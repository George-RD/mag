use std::sync::Arc;

use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::storage::{ReembedOptions, SqliteStorage};
use mag::memory_core::{
    EmbeddingInputKind, EmbeddingModel, MemoryInput, MemoryUpdate,
};
use rusqlite::Connection;
use tempfile::tempdir;

#[derive(Debug)]
struct FixedEmbeddingModel {
    identity: &'static str,
    dimension: usize,
}

impl FixedEmbeddingModel {
    fn new(identity: &'static str, dimension: usize) -> Self {
        Self {
            identity,
            dimension,
        }
    }
}

impl EmbeddingModel for FixedEmbeddingModel {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embedding_space_identity(&self) -> &str {
        self.identity
    }

    fn embed_for(&self, _input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0; self.dimension];
        if let Some(first) = embedding.first_mut() {
            *first = text.len() as f32;
        }
        Ok(embedding)
    }
}

fn assert_embedding_space_mismatch(error: &anyhow::Error) {
    assert!(
        error
            .chain()
            .any(|cause| cause.to_string().contains("embedding space mismatch")),
        "expected embedding-space mismatch, got: {error:#}"
    );
}

#[tokio::test]
async fn runtime_opened_before_reembed_cannot_write_after_same_dimension_migration() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(FixedEmbeddingModel::new("space-a", 4));
    let target: Arc<dyn EmbeddingModel> = Arc::new(FixedEmbeddingModel::new("space-b", 4));

    let stale_storage = SqliteStorage::new_with_path_and_embedding_model(
        path.clone(),
        Arc::clone(&source),
    )?;
    let stale_runtime = LocalMemoryRuntime::from_storage(stale_storage);
    stale_runtime
        .store_raw("alpha", "alpha memory", &MemoryInput::default())
        .await?;

    LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.clone(),
        Arc::clone(&target),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await?;

    let store_error = stale_runtime
        .store_raw("stale-store", "source-space store", &MemoryInput::default())
        .await
        .expect_err("a runtime opened on the source space must be invalid after migration");
    assert_embedding_space_mismatch(&store_error);

    let update_error = stale_runtime
        .update(
            "alpha",
            &MemoryUpdate {
                content: Some("source-space update".to_string()),
                ..MemoryUpdate::default()
            },
        )
        .await
        .expect_err("a stale runtime must not update an embedding after migration");
    assert_embedding_space_mismatch(&update_error);

    let stale_batch = vec![(
        "stale-batch".to_string(),
        "source-space batch".to_string(),
        MemoryInput::default(),
    )];
    let batch_error = stale_runtime
        .store_batch_raw(&stale_batch)
        .await
        .expect_err("a stale runtime must not batch-store embeddings after migration");
    assert_embedding_space_mismatch(&batch_error);

    let conn = Connection::open(&path)?;
    let alpha_content: String = conn.query_row(
        "SELECT content FROM memories WHERE id = 'alpha'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(alpha_content, "alpha memory");
    let stale_rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE id IN ('stale-store', 'stale-batch')",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(stale_rows, 0);
    drop(conn);

    let target_storage =
        SqliteStorage::new_with_path_and_embedding_model(path, Arc::clone(&target))?;
    let target_runtime = LocalMemoryRuntime::from_storage(target_storage);
    target_runtime
        .store_raw("fresh", "target-space store", &MemoryInput::default())
        .await?;

    Ok(())
}
