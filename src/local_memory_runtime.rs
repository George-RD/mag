use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::memory_core::storage::{InitMode, SqliteStorage};
use crate::memory_core::{
    AdvancedSearcher, Deleter, Embedder, GraphNode, GraphTraverser, ListResult, Lister,
    MemoryInput, MemoryUpdate, PhraseSearcher, Pipeline, PlaceholderPipeline, Relationship,
    RelationshipQuerier, SearchOptions, SearchResult, SemanticResult, SimilarFinder,
    VersionChainQuerier,
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

    /// Lists stored memories without changing pagination or filter semantics.
    pub async fn list(
        &self,
        offset: usize,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<ListResult> {
        self.storage.list(offset, limit, options).await
    }

    /// Returns stored relationships without changing ordering or payload semantics.
    pub async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>> {
        self.storage.get_relationships(memory_id).await
    }

    /// Traverses the relationship graph without changing hop, weight, or edge semantics.
    pub async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>> {
        self.storage
            .traverse(start_id, max_hops, min_weight, edge_types)
            .await
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

    /// Lists recent memories without changing filters, ordering, or result fields.
    pub async fn recent(&self, limit: usize, options: &SearchOptions) -> Result<Vec<SearchResult>> {
        self.compatibility_pipeline.recent(limit, options).await
    }

    /// Runs the current SQLite phrase-search implementation.
    pub async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.storage.phrase_search(phrase, limit, options).await
    }

    /// Runs the current semantic-search implementation.
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        options: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.compatibility_pipeline
            .semantic_search(query, limit, options)
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

    /// Returns the current version chain without changing ordering or metadata semantics.
    pub async fn version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>> {
        self.storage.get_version_chain(memory_id).await
    }

    /// Finds similar memories without changing scoring, filtering, or result fields.
    pub async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        self.storage.find_similar(memory_id, limit).await
    }
}

/// Builds the compatibility pipeline retained inside [`LocalMemoryRuntime`].
///
/// Keep this legacy assembly private to the selected composition root while its
/// delegates are replaced in later bounded slices.
fn compose_compatibility_pipeline(storage: &SqliteStorage) -> Pipeline {
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
