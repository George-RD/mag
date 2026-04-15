use schemars::JsonSchema;
use serde::Deserialize;

// ──────────────────────── Request structs ────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StoreRequest {
    pub content: String,
    pub id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i32>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub ttl_seconds: Option<i64>,
    /// ISO 8601 timestamp for when the event actually occurred (overrides default event_at).
    pub referenced_date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StoreBatchRequest {
    pub items: Vec<StoreRequest>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RetrieveRequest {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SearchRequest {
    /// Search mode: "text" (default, FTS5), "semantic" (embedding similarity), "phrase" (exact substring), "tag" (AND-match tags), "similar" (find similar to memory_id).
    pub mode: Option<String>,
    /// When true, run advanced multi-phase retrieval (supported for text/semantic modes only).
    /// Text mode defaults to advanced=true; set advanced=false to force the plain FTS5 path.
    pub advanced: Option<bool>,
    /// Query string (required for text, semantic, phrase modes).
    pub query: Option<String>,
    /// Tags to match (required for tag mode, AND logic).
    pub tags: Option<Vec<String>>,
    /// Source memory ID (required for similar mode).
    pub memory_id: Option<String>,
    pub limit: Option<usize>,
    pub event_type: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    pub event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    pub event_before: Option<String>,
    pub importance_min: Option<f64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub context_tags: Option<Vec<String>>,
    /// When true, inject component scores into each result's metadata under `_explain`.
    pub explain: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListRequest {
    /// Sort order: "created" (default, paginated by creation time) or "recent" (recently accessed).
    pub sort: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub event_type: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    pub event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    pub event_before: Option<String>,
    pub importance_min: Option<f64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub context_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DeleteRequest {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateRequest {
    pub id: String,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<String>,
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RelationsRequest {
    /// Action: "list" (default), "add", "traverse", "version_chain".
    pub action: Option<String>,
    /// Memory ID (required for list, traverse, version_chain).
    pub id: Option<String>,
    /// Source memory ID (required for add).
    pub source_id: Option<String>,
    /// Target memory ID (required for add).
    pub target_id: Option<String>,
    /// Relationship type (required for add).
    pub rel_type: Option<String>,
    /// Relationship weight (for add, default 1.0).
    pub weight: Option<f64>,
    /// Additional metadata (for add).
    pub metadata: Option<serde_json::Value>,
    /// Max hops for traverse (default 2).
    pub max_hops: Option<usize>,
    /// Min weight threshold for traverse (default 0.0).
    pub min_weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FeedbackRequest {
    pub memory_id: String,
    pub rating: String,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LifecycleRequest {
    /// Action: "sweep" (default, TTL expiration), "health", "consolidate", "compact", "auto_compact", "clear_session".
    pub action: Option<String>,
    pub warn_mb: Option<f64>,
    pub critical_mb: Option<f64>,
    pub max_nodes: Option<i64>,
    pub prune_days: Option<i64>,
    pub max_summaries: Option<i64>,
    pub event_type: Option<String>,
    pub similarity_threshold: Option<f64>,
    pub min_cluster_size: Option<usize>,
    pub dry_run: Option<bool>,
    pub session_id: Option<String>,
    /// Memory count threshold for auto_compact (default 500).
    pub count_threshold: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CheckpointRequest {
    /// Action: "save" (default) or "resume".
    pub action: Option<String>,
    /// Task title (required for save, optional filter for resume).
    pub task_title: Option<String>,
    /// Progress description (required for save).
    pub progress: Option<String>,
    pub plan: Option<String>,
    pub files_touched: Option<serde_json::Value>,
    pub decisions: Option<Vec<String>>,
    pub key_context: Option<String>,
    pub next_steps: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    /// Number of checkpoints to return (for resume, default 1).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RemindRequest {
    pub action: Option<String>,
    pub text: Option<String>,
    pub duration: Option<String>,
    pub context: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub status: Option<String>,
    pub reminder_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LessonsRequest {
    pub task: Option<String>,
    pub project: Option<String>,
    pub limit: Option<usize>,
    pub exclude_session: Option<String>,
    pub agent_type: Option<String>,
}

// AdminRequest removed — absorbed by MemoryAdminFacadeRequest

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionInfoRequest {
    /// Mode: "welcome" (default) or "protocol".
    pub mode: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ProfileRequest {
    pub action: Option<String>,
    pub update: Option<serde_json::Value>,
}

/// Unified facade request — routes to store / store_batch / retrieve / delete
/// based on `action` (default: "store").  Preview tool for Wave 2 MCP collapse.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemoryRequest {
    /// Action to perform: "store" (default), "store_batch", "retrieve", "delete".
    pub action: Option<String>,
    // ── store fields (all optional; required only for action=store) ──
    pub content: Option<String>,
    pub id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i32>,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub ttl_seconds: Option<i64>,
    /// ISO 8601 timestamp for when the event actually occurred (overrides default event_at).
    pub referenced_date: Option<String>,
    // ── store_batch fields ──
    /// Items for action=store_batch.
    pub items: Option<Vec<StoreRequest>>,
}

/// Unified facade for update / feedback / relations / lifecycle.
/// Routes based on `action` (default: "update").
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemoryManageRequest {
    /// Action: "update" (default), "feedback", "relations", "lifecycle".
    pub action: Option<String>,
    // ── update fields ──
    /// Memory ID (required for update, feedback, relations).
    pub id: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub event_type: Option<String>,
    pub priority: Option<i32>,
    // ── feedback fields ──
    pub memory_id: Option<String>,
    pub rating: Option<String>,
    pub reason: Option<String>,
    // ── relations fields ──
    /// Sub-action for action=relations: "list" (default), "add", "traverse", "version_chain".
    pub relations_action: Option<String>,
    pub source_id: Option<String>,
    pub target_id: Option<String>,
    pub rel_type: Option<String>,
    pub weight: Option<f64>,
    pub max_hops: Option<usize>,
    pub min_weight: Option<f64>,
    // ── lifecycle fields ──
    /// Sub-action for action=lifecycle: "sweep" (default), "health", "consolidate", "compact", "auto_compact", "clear_session", "backup", "backup_list".
    pub lifecycle_action: Option<String>,
    pub warn_mb: Option<f64>,
    pub critical_mb: Option<f64>,
    pub max_nodes: Option<i64>,
    pub prune_days: Option<i64>,
    pub max_summaries: Option<i64>,
    pub similarity_threshold: Option<f64>,
    pub min_cluster_size: Option<usize>,
    pub dry_run: Option<bool>,
    pub session_id: Option<String>,
    pub count_threshold: Option<usize>,
}

/// Unified facade for session_info / checkpoint / remind / lessons / profile.
/// Routes based on `action` (default: "info").
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemorySessionRequest {
    /// Action: "info" (default), "checkpoint", "remind", "lessons", "profile".
    pub action: Option<String>,
    // ── info fields ──
    /// Mode for action=info: "welcome" (default) or "protocol".
    pub info_mode: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    // ── checkpoint fields ──
    /// Sub-action for action=checkpoint: "save" (default) or "resume".
    pub checkpoint_action: Option<String>,
    pub task_title: Option<String>,
    pub progress: Option<String>,
    pub plan: Option<String>,
    pub files_touched: Option<serde_json::Value>,
    pub decisions: Option<Vec<String>>,
    pub key_context: Option<String>,
    pub next_steps: Option<String>,
    pub limit: Option<usize>,
    // ── remind fields ──
    /// Sub-action for action=remind: "set" (default), "list", "dismiss".
    pub remind_action: Option<String>,
    pub text: Option<String>,
    pub duration: Option<String>,
    pub context: Option<String>,
    pub status: Option<String>,
    pub reminder_id: Option<String>,
    // ── lessons fields ──
    pub task: Option<String>,
    pub exclude_session: Option<String>,
    pub agent_type: Option<String>,
    // ── profile fields ──
    /// Sub-action for action=profile: "read" (default) or "update".
    pub profile_action: Option<String>,
    pub update: Option<serde_json::Value>,
}

/// Unified facade for list / health / export / import.
/// Routes based on `action` (default: "health").
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemoryAdminFacadeRequest {
    /// Action: "health" (default), "list", "export", "import".
    pub action: Option<String>,
    // ── list fields ──
    /// Sort order: "created" (default) or "recent".
    pub sort: Option<String>,
    pub offset: Option<usize>,
    /// Result limit for action=list (default 10).
    pub list_limit: Option<usize>,
    /// Filter by event type for action=list.
    pub list_event_type: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub include_superseded: Option<bool>,
    pub event_after: Option<String>,
    pub event_before: Option<String>,
    pub importance_min: Option<f64>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub context_tags: Option<Vec<String>>,
    // ── health fields ──
    /// Detail level for action=health: "basic" (default), "stats", "types", "sessions", "digest", "access_rate".
    pub detail: Option<String>,
    /// Days for health detail=digest (default 7).
    pub days: Option<i64>,
    // ── import fields ──
    /// JSON data to import for action=import.
    pub data: Option<String>,
}
