use std::sync::Arc;

use anyhow::Result;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{EmbeddingInputKind, EmbeddingModel};
use tempfile::tempdir;

struct IdentifiedEmbedder {
    identity: &'static str,
}

impl EmbeddingModel for IdentifiedEmbedder {
    fn dimension(&self) -> usize {
        2
    }

    fn embedding_space_identity(&self) -> &str {
        self.identity
    }

    fn embed_for(&self, _input: EmbeddingInputKind, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![1.0, 0.0])
    }
}

#[test]
fn sqlite_persists_embedding_space_identity_on_first_open() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("memory.db");

    let storage = SqliteStorage::new_with_path_and_embedding_model(
        path.clone(),
        Arc::new(IdentifiedEmbedder {
            identity: "test-space-a",
        }),
    )
    .unwrap();
    drop(storage);

    let reopened = SqliteStorage::new_with_path_and_embedding_model(
        path,
        Arc::new(IdentifiedEmbedder {
            identity: "test-space-a",
        }),
    );
    assert!(reopened.is_ok());
}

#[test]
fn sqlite_rejects_a_different_embedding_space_with_the_same_dimension() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("memory.db");

    let storage = SqliteStorage::new_with_path_and_embedding_model(
        path.clone(),
        Arc::new(IdentifiedEmbedder {
            identity: "test-space-a",
        }),
    )
    .unwrap();
    drop(storage);

    let err = SqliteStorage::new_with_path_and_embedding_model(
        path,
        Arc::new(IdentifiedEmbedder {
            identity: "test-space-b",
        }),
    )
    .err()
    .expect("opening with an incompatible embedding space must fail");

    let message = err.to_string();
    assert!(
        message.contains("embedding space"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("test-space-a"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("test-space-b"),
        "unexpected error: {message}"
    );
}
