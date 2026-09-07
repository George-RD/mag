#![cfg(feature = "real-embeddings")]

use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;

use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{EmbeddingInputKind, EmbeddingModel, MemoryInput};
use rusqlite::Connection;

#[derive(Debug)]
struct SourceEmbeddingModel;

impl EmbeddingModel for SourceEmbeddingModel {
    fn dimension(&self) -> usize {
        384
    }

    fn embedding_space_identity(&self) -> &str {
        "source-profile:v1"
    }

    fn embed_for(&self, _input: EmbeddingInputKind, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; self.dimension()])
    }
}

fn run_cli(home: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_mag"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("MAG_DATA_ROOT", home.join(".mag"))
        .output()?)
}

fn persisted_embedding_space(home: &Path) -> Result<String> {
    let connection = Connection::open(home.join(".mag").join("memory.db"))?;
    Ok(connection.query_row(
        "SELECT value FROM runtime_metadata WHERE key = 'embedding_space_identity'",
        [],
        |row| row.get(0),
    )?)
}

#[tokio::test]
async fn cli_uses_the_same_pinned_profile_for_normal_startup_and_reembed() -> Result<()> {
    let normal_home = tempfile::tempdir()?;
    let list = run_cli(normal_home.path(), &["list", "--limit", "1"])?;
    anyhow::ensure!(
        list.status.success(),
        "list failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );
    let normal_identity = persisted_embedding_space(normal_home.path())?;
    assert!(normal_identity.starts_with("retriever-profile:v1"));
    assert!(!normal_identity.starts_with("legacy-role-neutral"));

    let migration_home = tempfile::tempdir()?;
    let database_path = migration_home.path().join(".mag").join("memory.db");
    let source: Arc<dyn EmbeddingModel> = Arc::new(SourceEmbeddingModel);
    let storage = SqliteStorage::new_with_path_and_embedding_model(database_path, source)?;
    let runtime = LocalMemoryRuntime::from_storage(storage);
    runtime
        .store_raw("alpha", "alpha memory", &MemoryInput::default())
        .await?;
    drop(runtime);

    let reembed = run_cli(migration_home.path(), &["re-embed", "--dry-run"])?;
    anyhow::ensure!(
        reembed.status.success(),
        "re-embed dry-run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&reembed.stdout),
        String::from_utf8_lossy(&reembed.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&reembed.stdout)?;
    assert_eq!(
        report["target_embedding_space"].as_str(),
        Some(normal_identity.as_str())
    );
    assert_eq!(report["source_embedding_space"], "source-profile:v1");
    assert_eq!(report["dry_run"], true);

    Ok(())
}
