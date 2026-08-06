use std::sync::{Arc, Mutex};

use anyhow::Result;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{
    AdvancedSearcher, EmbeddingInputKind, EmbeddingModel, MemoryInput, MemoryUpdate, SearchOptions,
    SemanticSearcher, Storage, Updater,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingCall {
    Single(EmbeddingInputKind),
    Batch(EmbeddingInputKind),
}

#[derive(Clone)]
struct RoleAwareOnlyEmbedder {
    calls: Arc<Mutex<Vec<EmbeddingCall>>>,
}

impl RoleAwareOnlyEmbedder {
    fn new(calls: Arc<Mutex<Vec<EmbeddingCall>>>) -> Self {
        Self { calls }
    }

    fn record(&self, call: EmbeddingCall) {
        self.calls.lock().expect("calls mutex poisoned").push(call);
    }
}

impl EmbeddingModel for RoleAwareOnlyEmbedder {
    fn dimension(&self) -> usize {
        2
    }

    fn embed_for(&self, input: EmbeddingInputKind, _text: &str) -> Result<Vec<f32>> {
        self.record(EmbeddingCall::Single(input));
        Ok(vec![1.0, 0.0])
    }

    fn embed_batch_for(
        &self,
        input: EmbeddingInputKind,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        self.record(EmbeddingCall::Batch(input));
        Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
    }
}

fn input(content: &str) -> MemoryInput {
    MemoryInput {
        content: content.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn sqlite_routes_embedding_inputs_by_role() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let embedder = Arc::new(RoleAwareOnlyEmbedder::new(Arc::clone(&calls)));
    let storage = SqliteStorage::new_in_memory_with_embedding_model(embedder).unwrap();

    Storage::store(&storage, "single", "single document", &input("single document"))
        .await
        .unwrap();

    storage
        .store_batch(&[
            (
                "batch-one".to_string(),
                "first batch document".to_string(),
                input("first batch document"),
            ),
            (
                "batch-two".to_string(),
                "second batch document".to_string(),
                input("second batch document"),
            ),
        ])
        .await
        .unwrap();

    Updater::update(
        &storage,
        "single",
        &MemoryUpdate {
            content: Some("updated document".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    SemanticSearcher::semantic_search(
        &storage,
        "find the updated document",
        5,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    AdvancedSearcher::advanced_search(
        &storage,
        "find the updated document",
        5,
        &SearchOptions::default(),
    )
    .await
    .unwrap();

    let observed = calls.lock().expect("calls mutex poisoned").clone();
    assert_eq!(
        observed
            .iter()
            .filter(|call| **call == EmbeddingCall::Single(EmbeddingInputKind::Document))
            .count(),
        2,
    );
    assert_eq!(
        observed
            .iter()
            .filter(|call| **call == EmbeddingCall::Batch(EmbeddingInputKind::Document))
            .count(),
        1,
    );
    assert_eq!(
        observed
            .iter()
            .filter(|call| **call == EmbeddingCall::Single(EmbeddingInputKind::Query))
            .count(),
        2,
    );
}
