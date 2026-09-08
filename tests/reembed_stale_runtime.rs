use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::storage::{ReembedOptions, SqliteStorage};
use mag::memory_core::{
    EmbeddingInputKind, EmbeddingModel, MemoryInput, MemoryUpdate, SearchOptions,
};
use rusqlite::Connection;
use tempfile::tempdir;

#[derive(Debug)]
struct FixedEmbeddingModel {
    identity: &'static str,
    dimension: usize,
    queries: AtomicUsize,
    query_gate: Mutex<Option<QueryGate>>,
}

#[derive(Debug)]
struct QueryGate {
    started: tokio::sync::oneshot::Sender<()>,
    resume: mpsc::Receiver<()>,
}

impl FixedEmbeddingModel {
    fn new(identity: &'static str, dimension: usize) -> Self {
        Self {
            identity,
            dimension,
            queries: AtomicUsize::new(0),
            query_gate: Mutex::new(None),
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

    fn embed_for(&self, input: EmbeddingInputKind, _text: &str) -> Result<Vec<f32>> {
        if input == EmbeddingInputKind::Query {
            self.queries.fetch_add(1, Ordering::SeqCst);
            let gate = self.query_gate.lock().expect("query gate poisoned").take();
            if let Some(gate) = gate {
                let _ = gate.started.send(());
                gate.resume.recv_timeout(Duration::from_secs(30))?;
            }
        }
        // Identical, normalized vectors make identity (not vector shape or value)
        // the only distinction between these two model spaces.
        let mut embedding = vec![0.0; self.dimension];
        if let Some(first) = embedding.first_mut() {
            *first = 1.0;
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

    let stale_storage =
        SqliteStorage::new_with_path_and_embedding_model(path.clone(), Arc::clone(&source))?;
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

    stale_runtime
        .update(
            "alpha",
            &MemoryUpdate {
                importance: Some(0.9),
                ..MemoryUpdate::default()
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

const READ_QUERY: &str = "What does alpha memory say about migration safety?";
const READ_CONTENT: &str =
    "alpha memory explains migration safety with consistent embedding spaces";

async fn migrate(path: &std::path::Path, identity: &'static str, dimension: usize) -> Result<()> {
    LocalMemoryRuntime::reembed_path_with_embedding_model(
        path.to_path_buf(),
        Arc::new(FixedEmbeddingModel::new(identity, dimension)),
        ReembedOptions {
            batch_size: 1,
            dry_run: false,
        },
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn stale_semantic_reads_fail_after_reembed_including_dimension_changes() -> Result<()> {
    for dimension in [4, 8] {
        let dir = tempdir()?;
        let path = dir.path().join("memory.db");
        let runtime =
            LocalMemoryRuntime::from_storage(SqliteStorage::new_with_path_and_embedding_model(
                path.clone(),
                Arc::new(FixedEmbeddingModel::new("space-a", 4)),
            )?);
        runtime
            .store_raw("alpha", READ_CONTENT, &MemoryInput::default())
            .await?;
        assert!(
            !runtime
                .semantic_search(READ_QUERY, 5, &SearchOptions::default())
                .await?
                .is_empty()
        );
        migrate(&path, "space-b", dimension).await?;
        let error = runtime
            .semantic_search(READ_QUERY, 5, &SearchOptions::default())
            .await
            .expect_err("stale semantic reads must fail visibly");
        assert_embedding_space_mismatch(&error);
        assert_eq!(runtime.retrieve("alpha").await?, READ_CONTENT);

        let fresh =
            LocalMemoryRuntime::from_storage(SqliteStorage::new_with_path_and_embedding_model(
                path.clone(),
                Arc::new(FixedEmbeddingModel::new("space-b", dimension)),
            )?);
        assert!(
            !fresh
                .semantic_search(READ_QUERY, 5, &SearchOptions::default())
                .await?
                .is_empty()
        );
    }
    Ok(())
}

#[tokio::test]
async fn stale_advanced_cache_hits_fail_after_reembed() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let model = Arc::new(FixedEmbeddingModel::new("space-a", 4));
    let runtime = LocalMemoryRuntime::from_storage(
        SqliteStorage::new_with_path_and_embedding_model(path.clone(), model.clone())?,
    );
    runtime
        .store_raw("alpha", READ_CONTENT, &MemoryInput::default())
        .await?;
    assert!(
        !runtime
            .advanced_search(READ_QUERY, 5, &SearchOptions::default())
            .await?
            .is_empty()
    );
    let queries = model.queries.load(Ordering::SeqCst);
    assert!(
        queries > 0,
        "fixture must exercise semantic advanced search"
    );
    runtime
        .advanced_search(READ_QUERY, 5, &SearchOptions::default())
        .await?;
    assert_eq!(
        model.queries.load(Ordering::SeqCst),
        queries,
        "fixture must exercise a cache hit"
    );
    migrate(&path, "space-b", 4).await?;
    let error = runtime
        .advanced_search(READ_QUERY, 5, &SearchOptions::default())
        .await
        .expect_err("stale cached advanced reads must fail visibly");
    assert_embedding_space_mismatch(&error);
    Ok(())
}

#[tokio::test]
async fn returning_to_original_space_does_not_revive_stale_query_caches() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let runtime =
        LocalMemoryRuntime::from_storage(SqliteStorage::new_with_path_and_embedding_model(
            path.clone(),
            Arc::new(FixedEmbeddingModel::new("space-a", 4)),
        )?);
    runtime
        .store_raw("alpha", READ_CONTENT, &MemoryInput::default())
        .await?;
    runtime
        .advanced_search(READ_QUERY, 5, &SearchOptions::default())
        .await?;
    migrate(&path, "space-b", 4).await?;
    migrate(&path, "space-a", 4).await?;
    let error = runtime
        .advanced_search(READ_QUERY, 5, &SearchOptions::default())
        .await
        .expect_err("round-trip migration must not revive an old generation");
    assert!(
        format!("{error:#}").contains("embedding generation mismatch"),
        "{error:#}"
    );
    let fresh = LocalMemoryRuntime::from_storage(SqliteStorage::new_with_path_and_embedding_model(
        path.clone(),
        Arc::new(FixedEmbeddingModel::new("space-a", 4)),
    )?);
    assert!(
        !fresh
            .advanced_search(READ_QUERY, 5, &SearchOptions::default())
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn migration_during_query_embedding_rejects_semantic_and_advanced_reads() -> Result<()> {
    for advanced in [false, true] {
        let dir = tempdir()?;
        let path = dir.path().join("memory.db");
        let model = Arc::new(FixedEmbeddingModel::new("space-a", 4));
        let runtime = LocalMemoryRuntime::from_storage(
            SqliteStorage::new_with_path_and_embedding_model(path.clone(), model.clone())?,
        );
        runtime
            .store_raw("alpha", READ_CONTENT, &MemoryInput::default())
            .await?;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        *model.query_gate.lock().expect("query gate poisoned") = Some(QueryGate {
            started: started_tx,
            resume: resume_rx,
        });
        let read = tokio::spawn(async move {
            if advanced {
                runtime
                    .advanced_search(READ_QUERY, 5, &SearchOptions::default())
                    .await
            } else {
                runtime
                    .semantic_search(READ_QUERY, 5, &SearchOptions::default())
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(30), started_rx).await??;
        let migration =
            tokio::time::timeout(Duration::from_secs(20), migrate(&path, "space-b", 4)).await;
        resume_tx.send(())?;
        migration??;
        let error = tokio::time::timeout(Duration::from_secs(30), read)
            .await??
            .expect_err("migration racing a semantic read must fail visibly");
        assert_embedding_space_mismatch(&error);
    }
    Ok(())
}

struct PausedReranker(Mutex<Option<QueryGate>>);

impl mag::memory_core::reranker::Reranker for PausedReranker {
    fn rerank(
        &self,
        _query: &str,
        _candidates: &[(&str, &str)],
    ) -> Result<std::collections::HashMap<String, f32>> {
        if let Some(gate) = self.0.lock().expect("reranker gate poisoned").take() {
            let _ = gate.started.send(());
            gate.resume.recv_timeout(Duration::from_secs(30))?;
        }
        Ok(std::collections::HashMap::new())
    }
}

#[tokio::test]
async fn round_trip_migration_between_candidate_and_fusion_phases_is_rejected() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("memory.db");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let storage = SqliteStorage::new_with_path_and_embedding_model(
        path.clone(),
        Arc::new(FixedEmbeddingModel::new("space-a", 4)),
    )?
    .with_reranker(Arc::new(PausedReranker(Mutex::new(Some(QueryGate {
        started: started_tx,
        resume: resume_rx,
    })))));
    let runtime = LocalMemoryRuntime::from_storage(storage);
    runtime
        .store_raw("alpha", READ_CONTENT, &MemoryInput::default())
        .await?;
    let read = tokio::spawn(async move {
        runtime
            .advanced_search(READ_QUERY, 5, &SearchOptions::default())
            .await
    });
    tokio::time::timeout(Duration::from_secs(30), started_rx).await??;
    let migration = tokio::time::timeout(Duration::from_secs(20), async {
        migrate(&path, "space-b", 4).await?;
        migrate(&path, "space-a", 4).await
    })
    .await;
    resume_tx.send(())?;
    migration??;
    let error = tokio::time::timeout(Duration::from_secs(30), read)
        .await??
        .expect_err("multi-phase reads must not mix embedding generations");
    assert!(
        format!("{error:#}").contains("embedding generation mismatch"),
        "{error:#}"
    );
    Ok(())
}
