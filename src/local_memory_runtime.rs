use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::memory_core::storage::{InitMode, SqliteStorage};
use crate::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, Embedder, GraphNode,
    GraphTraverser, LessonQuerier, ListResult, Lister, MemoryInput, MemoryUpdate, PhraseSearcher,
    Pipeline, PlaceholderPipeline, ProfileManager, Relationship, RelationshipQuerier,
    ReminderManager, SearchOptions, SearchResult, SemanticResult, SimilarFinder, StatsProvider,
    VersionChainQuerier, WelcomeOptions, WelcomeProvider,
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

    /// Saves a checkpoint without changing its content, metadata, or numbering semantics.
    pub async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String> {
        self.storage.save_checkpoint(input).await
    }

    /// Returns checkpoint continuity without changing filters, ordering, or payload fields.
    pub async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        self.storage.resume_task(query, project, limit).await
    }

    /// Creates a reminder without changing duration, metadata, or timestamp semantics.
    pub async fn create_reminder(
        &self,
        text: &str,
        duration: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.storage
            .create_reminder(text, duration, context, session_id, project)
            .await
    }

    /// Lists reminders without changing status filters, ordering, or payload fields.
    pub async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>> {
        self.storage.list_reminders(status).await
    }

    /// Dismisses a reminder without changing metadata or timestamp semantics.
    pub async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value> {
        self.storage.dismiss_reminder(reminder_id).await
    }

    /// Queries lessons without changing filtering, ordering, deduplication, or payload fields.
    pub async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        self.storage
            .query_lessons(task, project, exclude_session, agent_type, limit)
            .await
    }

    /// Returns the augmented user profile without changing stored or derived fields.
    pub async fn get_profile(&self) -> Result<serde_json::Value> {
        self.storage.get_profile().await
    }

    /// Applies profile updates without changing validation or merge semantics.
    pub async fn set_profile(&self, updates: &serde_json::Value) -> Result<()> {
        self.storage.set_profile(updates).await
    }

    /// Builds scoped session-start context without changing filtering or budget semantics.
    pub async fn welcome_scoped(&self, options: &WelcomeOptions) -> Result<serde_json::Value> {
        self.storage.welcome_scoped(options).await
    }

    /// Returns aggregate storage statistics without changing path or count fields.
    pub async fn stats(&self) -> Result<serde_json::Value> {
        self.storage.stats().await
    }

    /// Exports the complete local store without changing JSON formatting or content.
    pub async fn export_all(&self) -> Result<String> {
        self.storage.export_all().await
    }

    /// Returns memory counts by event type without changing aggregate fields.
    pub async fn type_stats(&self) -> Result<serde_json::Value> {
        self.storage.type_stats().await
    }

    /// Returns per-session statistics without changing ordering or payload fields.
    pub async fn session_stats(&self) -> Result<serde_json::Value> {
        self.storage.session_stats().await
    }

    /// Returns the current period digest without changing period or growth semantics.
    pub async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        self.storage.weekly_digest(days).await
    }

    /// Returns access-rate statistics without changing percentages or rankings.
    pub async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        self.storage.access_rate_stats().await
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
