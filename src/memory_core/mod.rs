use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

pub mod embedder;
pub mod scoring;
pub mod storage;

#[cfg(feature = "real-embeddings")]
#[allow(unused_imports)]
pub use embedder::OnnxEmbedder;
#[allow(unused_imports)]
pub use embedder::{Embedder, PlaceholderEmbedder};
#[allow(unused_imports)]
pub use scoring::{
    ABSTENTION_MIN_TEXT, GRAPH_MIN_EDGE_WEIGHT, GRAPH_NEIGHBOR_FACTOR, RRF_WEIGHT_FTS,
    RRF_WEIGHT_VEC, feedback_factor, jaccard_similarity, priority_factor, time_decay, type_weight,
    word_overlap,
};

pub const TTL_EPHEMERAL: i64 = 3600;
pub const TTL_SHORT_TERM: i64 = 86_400;
pub const TTL_LONG_TERM: i64 = 1_209_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Episodic,
    Semantic,
}

#[derive(Debug, Clone)]
pub struct MemoryInput {
    pub content: String,
    pub id: Option<String>,
    pub tags: Vec<String>,
    pub importance: f64,
    pub metadata: serde_json::Value,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i32>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub ttl_seconds: Option<i64>,
}

impl Default for MemoryInput {
    fn default() -> Self {
        Self {
            content: String::new(),
            id: None,
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
            priority: None,
            entity_id: None,
            agent_type: None,
            ttl_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryUpdate {
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<String>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub event_type: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub importance_min: Option<f64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub context_tags: Option<Vec<String>>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckpointInput {
    pub task_title: String,
    pub progress: String,
    pub plan: Option<String>,
    pub files_touched: Option<serde_json::Value>,
    pub decisions: Option<Vec<String>>,
    pub key_context: Option<String>,
    pub next_steps: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
}

pub const VALID_EVENT_TYPES: &[&str] = &[
    "session_summary",
    "task_completion",
    "error_pattern",
    "lesson_learned",
    "decision",
    "blocked_context",
    "user_preference",
    "advisor_insight",
    "git_commit",
    "git_merge",
    "git_conflict",
    "session_start",
    "session_end",
    "context_warning",
    "budget_alert",
    "coordination_snapshot",
    "checkpoint",
    "reminder",
    "memory",
    "code_chunk",
    "file_summary",
];

pub fn is_valid_event_type(event_type: &str) -> bool {
    VALID_EVENT_TYPES.contains(&event_type)
}

pub fn memory_kind_for_event_type(event_type: &str) -> MemoryKind {
    match event_type {
        "error_pattern" | "lesson_learned" | "user_preference" | "git_conflict" | "reminder"
        | "decision" => MemoryKind::Semantic,
        _ => MemoryKind::Episodic,
    }
}

pub fn default_priority_for_event_type(event_type: &str) -> i32 {
    match event_type {
        "error_pattern" | "lesson_learned" | "user_preference" | "git_conflict" => 4,
        "decision" | "task_completion" | "advisor_insight" => 3,
        "git_commit" | "git_merge" | "session_end" | "budget_alert" => 2,
        "session_summary" | "session_start" | "context_warning" | "coordination_snapshot" => 1,
        "blocked_context" | "checkpoint" | "reminder" | "memory" | "file_summary" => 1,
        "code_chunk" => 0,
        _ => 0,
    }
}

pub fn default_ttl_for_event_type(event_type: &str) -> Option<i64> {
    match event_type {
        "session_summary" => Some(TTL_EPHEMERAL),
        "task_completion" => Some(TTL_LONG_TERM),
        "error_pattern" => None,
        "lesson_learned" => None,
        "decision" => Some(TTL_LONG_TERM),
        "blocked_context" => Some(TTL_SHORT_TERM),
        "user_preference" => None,
        "advisor_insight" => Some(TTL_LONG_TERM),
        "git_commit" => Some(TTL_LONG_TERM),
        "git_merge" => Some(TTL_LONG_TERM),
        "git_conflict" => None,
        "session_start" => Some(TTL_SHORT_TERM),
        "session_end" => Some(TTL_LONG_TERM),
        "context_warning" => Some(TTL_SHORT_TERM),
        "budget_alert" => Some(TTL_LONG_TERM),
        "coordination_snapshot" => Some(TTL_SHORT_TERM),
        "checkpoint" => Some(604_800),
        "reminder" => None,
        "memory" => Some(TTL_SHORT_TERM),
        "code_chunk" => Some(TTL_EPHEMERAL),
        "file_summary" => Some(TTL_SHORT_TERM),
        _ => Some(TTL_LONG_TERM),
    }
}

pub fn parse_duration(text: &str) -> Result<chrono::Duration> {
    if text.is_empty() {
        return Err(anyhow::anyhow!("duration cannot be empty"));
    }

    let mut weeks: i64 = 0;
    let mut days: i64 = 0;
    let mut hours: i64 = 0;
    let mut minutes: i64 = 0;
    let mut last_rank: i32 = -1;
    let mut idx: usize = 0;
    let bytes = text.as_bytes();

    while idx < bytes.len() {
        if !bytes[idx].is_ascii_digit() {
            return Err(anyhow::anyhow!("invalid duration format: {text}"));
        }

        let start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }

        if idx >= bytes.len() {
            return Err(anyhow::anyhow!("invalid duration format: {text}"));
        }

        let value = text[start..idx]
            .parse::<i64>()
            .map_err(|_| anyhow::anyhow!("invalid duration value: {text}"))?;
        let unit = bytes[idx] as char;
        idx += 1;

        let rank = match unit {
            'w' => 0,
            'd' => 1,
            'h' => 2,
            'm' => 3,
            _ => return Err(anyhow::anyhow!("invalid duration unit in: {text}")),
        };

        if rank <= last_rank {
            return Err(anyhow::anyhow!("invalid duration order in: {text}"));
        }
        last_rank = rank;

        match unit {
            'w' => weeks = value,
            'd' => days = value,
            'h' => hours = value,
            'm' => minutes = value,
            _ => return Err(anyhow::anyhow!("invalid duration unit in: {text}")),
        }
    }

    let total = chrono::Duration::weeks(weeks)
        + chrono::Duration::days(days)
        + chrono::Duration::hours(hours)
        + chrono::Duration::minutes(minutes);

    if total.num_seconds() <= 0 {
        return Err(anyhow::anyhow!("duration must be greater than zero"));
    }

    Ok(total)
}

/// Search result item returned by memory queries.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// Memory identifier.
    pub id: String,
    /// Stored memory content.
    pub content: String,
    /// Memory tags.
    pub tags: Vec<String>,
    /// Importance score in the range [0.0, 1.0].
    pub importance: f64,
    /// Arbitrary JSON metadata payload.
    pub metadata: serde_json::Value,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
}

/// Semantic search result item with similarity score.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticResult {
    /// Memory identifier.
    pub id: String,
    /// Stored memory content.
    pub content: String,
    /// Memory tags.
    pub tags: Vec<String>,
    /// Importance score in the range [0.0, 1.0].
    pub importance: f64,
    /// Arbitrary JSON metadata payload.
    pub metadata: serde_json::Value,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    /// Similarity score in the range [0.0, 1.0].
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub content: String,
    pub event_type: Option<String>,
    pub metadata: serde_json::Value,
    pub hop: usize,
    pub weight: f64,
    pub edge_type: String,
    pub created_at: String,
}

/// A directed relationship between two memories.
#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    /// Relationship identifier.
    pub id: String,
    /// Source memory identifier.
    pub source_id: String,
    /// Target memory identifier.
    pub target_id: String,
    /// Relationship type label (e.g. "links_to", "related").
    pub rel_type: String,
    pub weight: f64,
    /// Arbitrary JSON metadata payload.
    pub metadata: serde_json::Value,
    pub created_at: String,
}

/// Result of a paginated list query.
#[derive(Debug, Clone, PartialEq)]
pub struct ListResult {
    /// Memories in the current page.
    pub memories: Vec<SearchResult>,
    /// Total number of memories in the store.
    pub total: usize,
}

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

/// Orchestrates the memory pipeline by coordinating ingestors, processors, and storage.
pub struct Pipeline {
    ingestor: Box<dyn Ingestor>,
    processor: Box<dyn Processor>,
    storage: Box<dyn Storage>,
    retriever: Box<dyn Retriever>,
    searcher: Box<dyn Searcher>,
    recents: Box<dyn Recents>,
    semantic_searcher: Box<dyn SemanticSearcher>,
}

impl Pipeline {
    /// Creates a new Pipeline with the provided components.
    pub fn new(
        ingestor: Box<dyn Ingestor>,
        processor: Box<dyn Processor>,
        storage: Box<dyn Storage>,
        retriever: Box<dyn Retriever>,
        searcher: Box<dyn Searcher>,
        recents: Box<dyn Recents>,
        semantic_searcher: Box<dyn SemanticSearcher>,
    ) -> Self {
        Self {
            ingestor,
            processor,
            storage,
            retriever,
            searcher,
            recents,
            semantic_searcher,
        }
    }

    /// Runs the full pipeline: ingest -> process -> store.
    pub async fn run(&self, content: &str, input: &MemoryInput) -> Result<String> {
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut store_input = input.clone();
        if store_input.id.is_none() {
            store_input.id = Some(id.clone());
        }
        let content_to_ingest = if content.is_empty() {
            input.content.as_str()
        } else {
            content
        };
        let ingested = self.ingestor.ingest(content_to_ingest).await?;
        let processed = self.processor.process(&ingested).await?;
        self.storage.store(&id, &processed, &store_input).await?;
        Ok(id)
    }

    /// Retrieves data from storage via the retriever.
    pub async fn retrieve(&self, id: &str) -> Result<String> {
        self.retriever.retrieve(id).await
    }

    /// Searches for stored memories matching the provided query.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.searcher.search(query, limit, opts).await
    }

    pub async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        self.recents.recent(limit, opts).await
    }

    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.semantic_searcher
            .semantic_search(query, limit, opts)
            .await
    }
}

/// A placeholder implementation of the memory pipeline for development and testing.
pub struct PlaceholderPipeline;

#[async_trait]
impl Ingestor for PlaceholderPipeline {
    async fn ingest(&self, content: &str) -> Result<String> {
        Ok(content.to_string())
    }
}

#[async_trait]
impl Processor for PlaceholderPipeline {
    async fn process(&self, input: &str) -> Result<String> {
        Ok(format!("processed: {}", input))
    }
}

#[async_trait]
impl Storage for PlaceholderPipeline {
    async fn store(&self, id: &str, data: &str, _input: &MemoryInput) -> Result<()> {
        info!(memory_id = %id, content_len = data.len(), "Storing placeholder payload");
        Ok(())
    }
}

#[async_trait]
impl Retriever for PlaceholderPipeline {
    async fn retrieve(&self, id: &str) -> Result<String> {
        Ok(format!("retrieved: {}", id))
    }
}

#[async_trait]
impl Searcher for PlaceholderPipeline {
    async fn search(
        &self,
        query: &str,
        _limit: usize,
        _opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        Ok(vec![SearchResult {
            id: "placeholder".to_string(),
            content: format!("search result for: {query}"),
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
        }])
    }
}

#[async_trait]
impl Recents for PlaceholderPipeline {
    async fn recent(&self, _limit: usize, _opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        Ok(vec![SearchResult {
            id: "placeholder-recent".to_string(),
            content: "recent result".to_string(),
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
        }])
    }
}

#[async_trait]
impl SemanticSearcher for PlaceholderPipeline {
    async fn semantic_search(
        &self,
        query: &str,
        _limit: usize,
        _opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        Ok(vec![SemanticResult {
            id: "placeholder-semantic".to_string(),
            content: format!("semantic result for: {query}"),
            tags: Vec::new(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            event_type: None,
            session_id: None,
            project: None,
            score: 1.0,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use serde_json::json;

    struct MockPipeline;

    #[async_trait]
    impl Ingestor for MockPipeline {
        async fn ingest(&self, content: &str) -> Result<String> {
            Ok(content.to_string())
        }
    }

    #[async_trait]
    impl Processor for MockPipeline {
        async fn process(&self, input: &str) -> Result<String> {
            Ok(format!("processed: {}", input))
        }
    }

    #[async_trait]
    impl Storage for MockPipeline {
        async fn store(&self, _id: &str, _data: &str, _input: &MemoryInput) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Retriever for MockPipeline {
        async fn retrieve(&self, id: &str) -> Result<String> {
            Ok(format!("retrieved: {}", id))
        }
    }

    #[async_trait]
    impl Searcher for MockPipeline {
        async fn search(
            &self,
            query: &str,
            _limit: usize,
            _opts: &SearchOptions,
        ) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "result-1".to_string(),
                content: format!("match: {query}"),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
            }])
        }
    }

    #[async_trait]
    impl Recents for MockPipeline {
        async fn recent(&self, _limit: usize, _opts: &SearchOptions) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "recent-1".to_string(),
                content: "recent value".to_string(),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
            }])
        }
    }

    #[async_trait]
    impl SemanticSearcher for MockPipeline {
        async fn semantic_search(
            &self,
            query: &str,
            _limit: usize,
            _opts: &SearchOptions,
        ) -> Result<Vec<SemanticResult>> {
            Ok(vec![SemanticResult {
                id: "semantic-1".to_string(),
                content: format!("semantic match: {query}"),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
                score: 0.99,
            }])
        }
    }

    struct FailingIngestor;

    #[async_trait]
    impl Ingestor for FailingIngestor {
        async fn ingest(&self, _content: &str) -> Result<String> {
            Err(anyhow!("Ingestion failed"))
        }
    }

    #[tokio::test]
    async fn test_ingestor_trait() {
        let ingestor: Box<dyn Ingestor> = Box::new(MockPipeline);
        let result = ingestor.ingest("test").await.unwrap();
        assert_eq!(result, "test");
    }

    #[tokio::test]
    async fn test_pipeline_run_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            id: Some("custom_id".to_string()),
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "custom_id");
    }

    #[tokio::test]
    async fn test_pipeline_run_default_id() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_ok());
        let id = result.unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_retrieve_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let result = pipeline.retrieve("test_id").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "retrieved: test_id");
    }

    #[tokio::test]
    async fn test_pipeline_failure() {
        let pipeline = Pipeline::new(
            Box::new(FailingIngestor),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Ingestion failed");
    }

    #[tokio::test]
    async fn test_pipeline_search_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline
            .search("needle", 5, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "result-1");
        assert_eq!(results[0].content, "match: needle");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
    }

    #[tokio::test]
    async fn test_pipeline_recent_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline.recent(3, &SearchOptions::default()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "recent-1");
        assert_eq!(results[0].content, "recent value");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
    }

    #[tokio::test]
    async fn test_pipeline_semantic_search_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline
            .semantic_search("vector", 4, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "semantic-1");
        assert_eq!(results[0].content, "semantic match: vector");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
        assert!(results[0].score > 0.9);
    }

    #[test]
    fn test_memory_kind_for_semantic_event_type() {
        assert_eq!(memory_kind_for_event_type("decision"), MemoryKind::Semantic);
    }

    #[test]
    fn test_memory_kind_defaults_to_episodic_for_unknown_type() {
        assert_eq!(
            memory_kind_for_event_type("totally_unknown"),
            MemoryKind::Episodic
        );
    }
}
