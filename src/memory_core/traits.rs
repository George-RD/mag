use anyhow::Result;
use async_trait::async_trait;

use super::domain::{
    BackupInfo, CheckpointInput, GraphNode, ListResult, MemoryInput, MemoryUpdate, Relationship,
    SearchOptions, SearchResult, SemanticResult, WelcomeOptions,
};

/// Trait for ingesting raw content into the memory system.
#[async_trait]
pub trait Ingestor: Send + Sync {
    /// Ingests the provided content and returns a processed string.
    async fn ingest(&self, content: &str) -> Result<String>;
}

/// Trait for processing ingested content (e.g., summarization, embedding).
#[async_trait]
pub trait Processor: Send + Sync {
    /// Processes the input string and returns a refined result.
    async fn process(&self, input: &str) -> Result<String>;
}

/// Trait for storing processed memory data.
#[async_trait]
pub trait Storage: Send + Sync {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()>;
}

/// Trait for retrieving stored memory data.
#[async_trait]
pub trait Retriever: Send + Sync {
    /// Retrieves the data associated with the given ID.
    async fn retrieve(&self, id: &str) -> Result<String>;
}

/// Trait for searching stored memory data.
#[async_trait]
pub trait Searcher: Send + Sync {
    /// Searches for memories matching the query string.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
}

#[async_trait]
pub trait Recents: Send + Sync {
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>>;
}

#[async_trait]
pub trait SemanticSearcher: Send + Sync {
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>>;
}

#[async_trait]
pub trait GraphTraverser: Send + Sync {
    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>>;
}

#[async_trait]
pub trait SimilarFinder: Send + Sync {
    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>>;
}

#[async_trait]
pub trait PhraseSearcher: Send + Sync {
    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
}

#[async_trait]
pub trait AdvancedSearcher: Send + Sync {
    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>>;
}

/// Trait for deleting stored memories.
#[async_trait]
pub trait Deleter: Send + Sync {
    /// Deletes the memory with the given ID. Returns `true` if a row was removed.
    async fn delete(&self, id: &str) -> Result<bool>;
}

/// Trait for updating stored memory content and tags.
#[async_trait]
pub trait Updater: Send + Sync {
    /// Updates an existing memory. At least one of content, tags, importance, or metadata must be provided.
    async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()>;
}

/// Trait for querying memories by tag.
#[async_trait]
pub trait Tagger: Send + Sync {
    /// Returns memories whose tags contain **all** of the supplied tags.
    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>>;
}

/// Trait for paginated listing of memories.
#[async_trait]
pub trait Lister: Send + Sync {
    /// Lists memories with pagination, returning the page and total count.
    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult>;
}

/// Trait for querying relationships of a memory.
#[async_trait]
pub trait RelationshipQuerier: Send + Sync {
    /// Returns all relationships where `memory_id` is either source or target.
    async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>>;
}

#[async_trait]
pub trait VersionChainQuerier: Send + Sync {
    /// Returns the full version chain for a memory, ordered by created_at ascending.
    /// Includes the memory itself and all versions in its chain.
    async fn get_version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>>;

    /// Manually supersede an old memory with a new one.
    /// Creates SUPERSEDES relationship and sets superseded_by_id on the old memory.
    #[allow(dead_code)]
    async fn supersede_memory(&self, old_id: &str, new_id: &str) -> Result<()>;
}

#[async_trait]
pub trait FeedbackRecorder: Send + Sync {
    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value>;
}

#[async_trait]
pub trait ExpirationSweeper: Send + Sync {
    async fn sweep_expired(&self) -> Result<usize>;
}

#[async_trait]
pub trait ProfileManager: Send + Sync {
    async fn get_profile(&self) -> Result<serde_json::Value>;
    async fn set_profile(&self, updates: &serde_json::Value) -> Result<()>;
}

#[async_trait]
pub trait CheckpointManager: Send + Sync {
    async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String>;
    async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>>;
}

#[async_trait]
pub trait ReminderManager: Send + Sync {
    async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;
    async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>>;
    async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value>;
}

#[async_trait]
pub trait LessonQuerier: Send + Sync {
    async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>>;
}

/// Maintenance operations for memory store housekeeping.
#[async_trait]
pub trait MaintenanceManager: Send + Sync {
    /// Check database health: size, integrity, node count vs limits.
    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value>;
    /// Prune zero-access memories older than `prune_days` and cap session summaries.
    async fn consolidate(&self, prune_days: i64, max_summaries: i64) -> Result<serde_json::Value>;
    /// Merge near-duplicate memories of a given event type using Jaccard similarity.
    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value>;
    /// Delete all memories (and their relationships) for a given session.
    async fn clear_session(&self, session_id: &str) -> Result<usize>;
    /// Auto-compact: sweep all event types that have dedup thresholds,
    /// using embedding cosine similarity. Triggered when memory count
    /// exceeds `count_threshold`. Returns summary of compacted clusters.
    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value>;
}

/// Session startup briefing provider.
#[async_trait]
pub trait WelcomeProvider: Send + Sync {
    /// Generate a welcome briefing with recent activity, profile, and pending reminders.
    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;

    /// Generate a scoped welcome briefing using structured options.
    /// Default implementation delegates to `welcome()` using session_id and project fields.
    async fn welcome_scoped(&self, opts: &WelcomeOptions) -> Result<serde_json::Value> {
        self.welcome(opts.session_id.as_deref(), opts.project.as_deref())
            .await
    }
}

/// Manages database backup, rotation, and restore.
#[async_trait]
pub trait BackupManager: Send + Sync {
    /// Create a binary backup of the database file, returning backup metadata.
    async fn create_backup(&self) -> Result<BackupInfo>;
    /// Rotate backups, keeping only the `max_count` most recent. Returns the number removed.
    async fn rotate_backups(&self, max_count: usize) -> Result<usize>;
    /// List available backups with path and size.
    async fn list_backups(&self) -> Result<Vec<BackupInfo>>;
    /// Restore from a backup file. Creates a safety backup of the current DB first.
    async fn restore_backup(&self, backup_path: &std::path::Path) -> Result<()>;
    /// Run automatic startup backup if needed (>24h since last). Returns backup path if created.
    async fn maybe_startup_backup(&self) -> Result<Option<BackupInfo>>;
}

/// Extended statistics beyond basic counts.
#[async_trait]
pub trait StatsProvider: Send + Sync {
    /// Per-event-type memory counts.
    async fn type_stats(&self) -> Result<serde_json::Value>;
    /// Per-session memory counts (top 20).
    async fn session_stats(&self) -> Result<serde_json::Value>;
    /// Activity digest for a given period.
    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value>;
    /// Access rate analysis: zero-access %, top-accessed, by-type breakdown.
    async fn access_rate_stats(&self) -> Result<serde_json::Value>;
}
