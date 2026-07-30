use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::memory_core::storage::{InitMode, SqliteStorage};
use crate::memory_core::{
    AdvancedSearcher, Deleter, Embedder, MemoryInput, MemoryUpdate, Pipeline, PlaceholderPipeline,
    SearchOptions, SearchResult, SemanticResult, Updater,
};

/// Transport-independent composition root for MAG's local memory capabilities.
///
/// The initial implementation deliberately delegates to the existing
/// compatibility pipeline and SQLite backend. It introduces one place to own
/// those components without changing their storage or retrieval semantics.
pub struct LocalMemoryRuntime {
    storage: SqliteStorage,
    compatibility_pipeline: Pipeline,
}

impl LocalMemoryRuntime {
    /// Opens the default local SQLite store and composes the current delegates.
    ///
    /// This performs blocking filesystem and SQLite initialization, matching
    /// [`SqliteStorage::new`]. Construct it before serving async requests or
    /// wrap the call in `tokio::task::spawn_blocking`.
    pub fn new(mode: InitMode, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let storage = SqliteStorage::new(mode, embedder)?;
        Ok(Self::from_storage(storage))
    }

    /// Opens a local SQLite store at `path` and composes the current delegates.
    ///
    /// This constructor exists for hermetic callers and tests. It performs
    /// blocking initialization, matching [`SqliteStorage::new_with_path`].
    pub fn new_with_path(path: PathBuf, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let storage = SqliteStorage::new_with_path(path, embedder)?;
        Ok(Self::from_storage(storage))
    }

    /// Composes a runtime around one existing SQLite store.
    ///
    /// `SqliteStorage` clones share the same connection pool, embedder, caches,
    /// and configuration. The compatibility pipeline therefore delegates to
    /// the same underlying store retained for extended capabilities.
    pub fn from_storage(storage: SqliteStorage) -> Self {
        let compatibility_pipeline = compose_compatibility_pipeline(&storage);

        Self {
            storage,
            compatibility_pipeline,
        }
    }

    /// Stores content through the compatibility-sensitive CLI pipeline.
    pub async fn store(&self, content: &str, input: &MemoryInput) -> Result<String> {
        self.compatibility_pipeline.run(content, input).await
    }

    /// Retrieves stored content without changing the current output.
    pub async fn retrieve(&self, id: &str) -> Result<String> {
        self.compatibility_pipeline.retrieve(id).await
    }

    /// Deletes stored content without changing the current boolean result.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.storage.delete(id).await
    }

    /// Updates stored fields without changing the current mutation semantics.
    pub async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        self.storage.update(id, input).await
    }

    /// Runs the current basic search implementation.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.compatibility_pipeline
            .search(query, limit, options)
            .await
    }

    /// Runs the current SQLite advanced-search implementation.
    pub async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.storage.advanced_search(query, limit, options).await
    }
}

/// Builds the temporary compatibility pipeline used by callers not yet moved
/// behind [`LocalMemoryRuntime`]. Keep this assembly in one place while the
/// remaining command families migrate.
pub(crate) fn compose_compatibility_pipeline(storage: &SqliteStorage) -> Pipeline {
    Pipeline::new(
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
        Box::new(storage.clone()),
    )
}
