use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

pub mod embedder;
pub mod reranker;
pub mod scoring;
pub mod storage;

#[cfg(feature = "real-embeddings")]
#[allow(unused_imports)]
pub use embedder::OnnxEmbedder;
#[allow(unused_imports)]
pub use embedder::{Embedder, PlaceholderEmbedder};
#[allow(unused_imports)]
pub(crate) use scoring::token_set;
#[allow(unused_imports)]
pub use scoring::{
    ABSTENTION_MIN_TEXT, GRAPH_MIN_EDGE_WEIGHT, GRAPH_NEIGHBOR_FACTOR, RRF_WEIGHT_FTS,
    RRF_WEIGHT_VEC, ScoringParams, feedback_factor, jaccard_pre, jaccard_similarity,
    priority_factor, time_decay_et, type_weight_et, word_overlap_pre,
};

pub const TTL_EPHEMERAL: i64 = 3600;
pub const TTL_SHORT_TERM: i64 = 86_400;
pub const TTL_LONG_TERM: i64 = 1_209_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Episodic,
    Semantic,
}

/// Strongly-typed event type for memories.
///
/// Serializes to/from its snake_case string representation for SQLite TEXT
/// column backward compatibility and MCP JSON protocol compatibility.
/// The `Unknown(String)` variant provides forward compatibility for
/// event types not yet defined in the enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventType {
    SessionSummary,
    TaskCompletion,
    ErrorPattern,
    LessonLearned,
    Decision,
    BlockedContext,
    UserPreference,
    UserFact,
    AdvisorInsight,
    GitCommit,
    GitMerge,
    GitConflict,
    SessionStart,
    SessionEnd,
    ContextWarning,
    BudgetAlert,
    CoordinationSnapshot,
    Checkpoint,
    Reminder,
    Memory,
    CodeChunk,
    FileSummary,
    /// Forward-compatibility variant for unknown event types.
    Unknown(String),
}

impl EventType {
    /// Returns `true` if this is a known (non-Unknown) event type.
    pub fn is_valid(&self) -> bool {
        !matches!(self, EventType::Unknown(_))
    }

    /// Returns the memory kind for this event type.
    pub fn memory_kind(&self) -> MemoryKind {
        match self {
            EventType::ErrorPattern
            | EventType::LessonLearned
            | EventType::UserPreference
            | EventType::GitConflict
            | EventType::Reminder
            | EventType::Decision => MemoryKind::Semantic,
            EventType::Unknown(_) => MemoryKind::Episodic,
            _ => MemoryKind::Episodic,
        }
    }

    /// Returns the default priority for this event type.
    pub fn default_priority(&self) -> i32 {
        match self {
            EventType::ErrorPattern
            | EventType::LessonLearned
            | EventType::UserPreference
            | EventType::GitConflict => 4,
            EventType::Decision | EventType::TaskCompletion | EventType::AdvisorInsight => 3,
            EventType::GitCommit
            | EventType::GitMerge
            | EventType::SessionEnd
            | EventType::BudgetAlert => 2,
            EventType::SessionSummary
            | EventType::SessionStart
            | EventType::ContextWarning
            | EventType::CoordinationSnapshot => 1,
            EventType::BlockedContext
            | EventType::Checkpoint
            | EventType::Reminder
            | EventType::Memory
            | EventType::FileSummary => 1,
            EventType::CodeChunk => 0,
            EventType::UserFact => 4,
            EventType::Unknown(_) => 0,
        }
    }

    /// Returns the default TTL for this event type.
    pub fn default_ttl(&self) -> Option<i64> {
        match self {
            EventType::SessionSummary => Some(TTL_EPHEMERAL),
            EventType::TaskCompletion => Some(TTL_LONG_TERM),
            EventType::ErrorPattern => None,
            EventType::LessonLearned => None,
            EventType::Decision => Some(TTL_LONG_TERM),
            EventType::BlockedContext => Some(TTL_SHORT_TERM),
            EventType::UserPreference => None,
            EventType::UserFact => None,
            EventType::AdvisorInsight => Some(TTL_LONG_TERM),
            EventType::GitCommit => Some(TTL_LONG_TERM),
            EventType::GitMerge => Some(TTL_LONG_TERM),
            EventType::GitConflict => None,
            EventType::SessionStart => Some(TTL_SHORT_TERM),
            EventType::SessionEnd => Some(TTL_LONG_TERM),
            EventType::ContextWarning => Some(TTL_SHORT_TERM),
            EventType::BudgetAlert => Some(TTL_LONG_TERM),
            EventType::CoordinationSnapshot => Some(TTL_SHORT_TERM),
            EventType::Checkpoint => Some(604_800),
            EventType::Reminder => None,
            EventType::Memory => Some(TTL_SHORT_TERM),
            EventType::CodeChunk => Some(TTL_EPHEMERAL),
            EventType::FileSummary => Some(TTL_SHORT_TERM),
            EventType::Unknown(_) => Some(TTL_LONG_TERM),
        }
    }

    /// Returns the type weight for search scoring.
    pub fn type_weight(&self) -> f64 {
        match self {
            EventType::Checkpoint => 2.5,
            EventType::Reminder => 3.0,
            EventType::Decision => 2.0,
            EventType::LessonLearned => 2.0,
            EventType::ErrorPattern => 2.0,
            EventType::UserPreference => 2.0,
            EventType::TaskCompletion => 1.4,
            EventType::SessionSummary => 1.2,
            EventType::BlockedContext => 1.0,
            EventType::GitCommit => 1.0,
            EventType::GitMerge => 1.0,
            EventType::GitConflict => 1.0,
            EventType::CoordinationSnapshot => 0.2,
            EventType::Memory => 1.0,
            _ => 1.0,
        }
    }

    /// Returns the dedup threshold for this event type, if applicable.
    pub fn dedup_threshold(&self) -> Option<f64> {
        match self {
            EventType::ErrorPattern => Some(0.70),
            EventType::SessionSummary => Some(0.75),
            EventType::TaskCompletion => Some(0.85),
            EventType::Decision => Some(0.80),
            EventType::LessonLearned => Some(0.85),
            _ => None,
        }
    }

    /// Returns `true` if this event type supports auto-supersession.
    pub fn is_supersession_type(&self) -> bool {
        matches!(
            self,
            EventType::Decision | EventType::LessonLearned | EventType::UserPreference
        )
    }

    /// Converts an optional string (from JSON/SQL) into `Option<EventType>`.
    pub fn from_optional(s: Option<&str>) -> Option<EventType> {
        s.map(|v| EventType::from_str(v).unwrap_or_else(|e| match e {}))
    }

    /// Returns all event types that have a dedup threshold.
    pub fn types_with_dedup_threshold() -> Vec<EventType> {
        // Keep in sync with dedup_threshold() match arms above.
        vec![
            EventType::ErrorPattern,
            EventType::SessionSummary,
            EventType::TaskCompletion,
            EventType::Decision,
            EventType::LessonLearned,
        ]
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventType::SessionSummary => "session_summary",
            EventType::TaskCompletion => "task_completion",
            EventType::ErrorPattern => "error_pattern",
            EventType::LessonLearned => "lesson_learned",
            EventType::Decision => "decision",
            EventType::BlockedContext => "blocked_context",
            EventType::UserPreference => "user_preference",
            EventType::UserFact => "user_fact",
            EventType::AdvisorInsight => "advisor_insight",
            EventType::GitCommit => "git_commit",
            EventType::GitMerge => "git_merge",
            EventType::GitConflict => "git_conflict",
            EventType::SessionStart => "session_start",
            EventType::SessionEnd => "session_end",
            EventType::ContextWarning => "context_warning",
            EventType::BudgetAlert => "budget_alert",
            EventType::CoordinationSnapshot => "coordination_snapshot",
            EventType::Checkpoint => "checkpoint",
            EventType::Reminder => "reminder",
            EventType::Memory => "memory",
            EventType::CodeChunk => "code_chunk",
            EventType::FileSummary => "file_summary",
            EventType::Unknown(s) => s.as_str(),
        };
        write!(f, "{s}")
    }
}

impl FromStr for EventType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "session_summary" => EventType::SessionSummary,
            "task_completion" => EventType::TaskCompletion,
            "error_pattern" => EventType::ErrorPattern,
            "lesson_learned" => EventType::LessonLearned,
            "decision" => EventType::Decision,
            "blocked_context" => EventType::BlockedContext,
            "user_preference" => EventType::UserPreference,
            "user_fact" => EventType::UserFact,
            "advisor_insight" => EventType::AdvisorInsight,
            "git_commit" => EventType::GitCommit,
            "git_merge" => EventType::GitMerge,
            "git_conflict" => EventType::GitConflict,
            "session_start" => EventType::SessionStart,
            "session_end" => EventType::SessionEnd,
            "context_warning" => EventType::ContextWarning,
            "budget_alert" => EventType::BudgetAlert,
            "coordination_snapshot" => EventType::CoordinationSnapshot,
            "checkpoint" => EventType::Checkpoint,
            "reminder" => EventType::Reminder,
            "memory" => EventType::Memory,
            "code_chunk" => EventType::CodeChunk,
            "file_summary" => EventType::FileSummary,
            other => EventType::Unknown(other.to_string()),
        })
    }
}

impl Serialize for EventType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EventType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // FromStr is infallible for EventType
        Ok(EventType::from_str(&s).unwrap_or_else(|e| match e {}))
    }
}

impl schemars::JsonSchema for EventType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("EventType")
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Memory event type",
            "enum": [
                "session_summary", "task_completion", "error_pattern", "lesson_learned",
                "decision", "blocked_context", "user_preference", "user_fact",
                "advisor_insight", "git_commit", "git_merge", "git_conflict",
                "session_start", "session_end", "context_warning", "budget_alert",
                "coordination_snapshot", "checkpoint", "reminder", "memory",
                "code_chunk", "file_summary"
            ]
        })
    }
}

#[derive(Debug, Clone)]
pub struct MemoryInput {
    pub content: String,
    pub id: Option<String>,
    pub tags: Vec<String>,
    pub importance: f64,
    pub metadata: serde_json::Value,
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i32>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub ttl_seconds: Option<i64>,
    /// ISO 8601 timestamp for when the event actually occurred.
    /// When provided, this overrides the default `event_at = now()` on insert.
    pub referenced_date: Option<String>,
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
            referenced_date: None,
        }
    }
}

impl MemoryInput {
    /// Sets `event_type` from `event_type_str` when provided, otherwise preserves the
    /// existing `self.event_type`. Then applies derived defaults for `ttl_seconds` and
    /// `priority` (only when those fields are `None`) based on the effective event type.
    pub fn apply_event_type_defaults(&mut self, event_type_str: Option<&str>) {
        let event_type =
            EventType::from_optional(event_type_str).or_else(|| self.event_type.clone());

        if self.ttl_seconds.is_none() {
            self.ttl_seconds = event_type
                .as_ref()
                .map(EventType::default_ttl)
                .unwrap_or(Some(TTL_LONG_TERM));
        }
        if self.priority.is_none() {
            self.priority = event_type.as_ref().map(EventType::default_priority);
        }
        self.event_type = event_type;
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryUpdate {
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<EventType>,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub event_type: Option<EventType>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub include_superseded: Option<bool>,
    pub importance_min: Option<f64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub context_tags: Option<Vec<String>>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    /// ISO 8601 lower bound for the `event_at` column (inclusive).
    pub event_after: Option<String>,
    /// ISO 8601 upper bound for the `event_at` column (inclusive).
    pub event_before: Option<String>,
    /// When true, inject `_explain` component scores into each result's metadata.
    pub explain: Option<bool>,
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

/// Checks if a string represents a known event type.
/// Thin wrapper that delegates to `EventType`.
pub fn is_valid_event_type(event_type: &str) -> bool {
    EventType::from_str(event_type)
        .map(|et| et.is_valid())
        .unwrap_or(false)
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
}

/// Semantic search result item with similarity score.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    /// Similarity score in the range [0.0, 1.0].
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub content: String,
    pub event_type: Option<EventType>,
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
        assert_eq!(EventType::Decision.memory_kind(), MemoryKind::Semantic);
    }

    #[test]
    fn test_memory_kind_defaults_to_episodic_for_unknown_type() {
        assert_eq!(
            EventType::Unknown("totally_unknown".to_string()).memory_kind(),
            MemoryKind::Episodic
        );
    }
}
