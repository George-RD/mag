use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const TTL_EPHEMERAL: i64 = 3600;
pub const TTL_SHORT_TERM: i64 = 86_400;
pub const TTL_LONG_TERM: i64 = 1_209_600;

// ── Relationship type constants ──────────────────────────────────────────
/// Temporal adjacency: the source memory was stored immediately before the target
/// within the same session.
pub const REL_PRECEDED_BY: &str = "PRECEDED_BY";
/// Entity co-occurrence: two memories share an entity tag.
pub const REL_RELATES_TO: &str = "RELATES_TO";
/// Semantic similarity detected at store time (auto-relate).
pub const REL_RELATED: &str = "related";
/// Alternate semantic-similarity labels used in graph scoring.
pub const REL_SIMILAR_TO: &str = "SIMILAR_TO";
pub const REL_SHARES_THEME: &str = "SHARES_THEME";
pub const REL_PARALLEL_CONTEXT: &str = "PARALLEL_CONTEXT";

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
            EventType::UserFact => Some(0.85),
            EventType::UserPreference => Some(0.75),
            _ => None,
        }
    }

    /// Returns `true` if this event type supports auto-supersession.
    pub fn is_supersession_type(&self) -> bool {
        matches!(
            self,
            EventType::Decision
                | EventType::LessonLearned
                | EventType::UserPreference
                | EventType::UserFact
                | EventType::Reminder
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
            EventType::UserFact,
            EventType::UserPreference,
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
    /// Source of the memory (e.g. "cli_input", "mcp", "import"). Defaults to "cli_input".
    pub source_type: Option<String>,
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
            source_type: None,
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
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
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
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
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

/// Backup info returned by `BackupManager::create_backup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub path: std::path::PathBuf,
    pub size_bytes: u64,
    pub created_at: String,
}
