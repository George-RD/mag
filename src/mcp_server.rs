use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, EventType, ExpirationSweeper,
    FeedbackRecorder, GraphTraverser, LessonQuerier, Lister, MaintenanceManager, MemoryInput,
    MemoryUpdate, PhraseSearcher, ProfileManager, Recents, RelationshipQuerier, ReminderManager,
    Retriever, SearchOptions, Searcher, SemanticSearcher, SimilarFinder, StatsProvider, Storage,
    Tagger, Updater, VersionChainQuerier, WelcomeProvider, is_valid_event_type,
};

/// Serialize a collection of items into a `Vec<serde_json::Value>`, returning
/// `McpError::internal_error` on the first serialization failure.
fn serialize_results<T: Serialize>(
    items: impl IntoIterator<Item = T>,
) -> Result<Vec<serde_json::Value>, McpError> {
    items
        .into_iter()
        .map(|item| {
            serde_json::to_value(&item).map_err(|e| {
                McpError::internal_error(format!("failed to serialize result: {e}"), None)
            })
        })
        .collect()
}

/// Convert a StoreRequest into (id, MemoryInput) with defaults applied.
/// Validates event_type so callers don't need to duplicate the check.
fn build_memory_input(item: &StoreRequest) -> Result<(String, MemoryInput), McpError> {
    if let Some(et) = item.event_type.as_deref()
        && !is_valid_event_type(et)
    {
        return Err(McpError::invalid_params("invalid event_type", None));
    }
    let id = item
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut input = MemoryInput {
        content: item.content.clone(),
        id: Some(id.clone()),
        tags: item.tags.clone().unwrap_or_default(),
        importance: item.importance.unwrap_or(0.5),
        metadata: item
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        priority: item.priority,
        session_id: item.session_id.clone(),
        project: item.project.clone(),
        entity_id: item.entity_id.clone(),
        agent_type: item.agent_type.clone(),
        ttl_seconds: item.ttl_seconds,
        referenced_date: item.referenced_date.clone(),
        ..MemoryInput::default()
    };
    input.apply_event_type_defaults(item.event_type.as_deref());
    Ok((id, input))
}

#[derive(Clone)]
pub struct McpMemoryServer {
    storage: SqliteStorage,
    tool_router: ToolRouter<Self>,
}

impl McpMemoryServer {
    pub fn new(storage: SqliteStorage) -> Self {
        Self {
            storage,
            tool_router: Self::tool_router(),
        }
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

// ──────────────────────── Request structs ────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct StoreRequest {
    content: String,
    id: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f64>,
    metadata: Option<serde_json::Value>,
    event_type: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
    priority: Option<i32>,
    entity_id: Option<String>,
    agent_type: Option<String>,
    ttl_seconds: Option<i64>,
    /// ISO 8601 timestamp for when the event actually occurred (overrides default event_at).
    referenced_date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StoreBatchRequest {
    items: Vec<StoreRequest>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RetrieveRequest {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchRequest {
    /// Search mode: "text" (default, FTS5), "semantic" (embedding similarity), "phrase" (exact substring), "tag" (AND-match tags), "similar" (find similar to memory_id).
    mode: Option<String>,
    /// When true, run advanced multi-phase retrieval (supported for text/semantic modes only).
    /// Text mode defaults to advanced=true; set advanced=false to force the plain FTS5 path.
    advanced: Option<bool>,
    /// Query string (required for text, semantic, phrase modes).
    query: Option<String>,
    /// Tags to match (required for tag mode, AND logic).
    tags: Option<Vec<String>>,
    /// Source memory ID (required for similar mode).
    memory_id: Option<String>,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
    importance_min: Option<f64>,
    created_after: Option<String>,
    created_before: Option<String>,
    context_tags: Option<Vec<String>>,
    /// When true, inject component scores into each result's metadata under `_explain`.
    explain: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListRequest {
    /// Sort order: "created" (default, paginated by creation time) or "recent" (recently accessed).
    sort: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
    importance_min: Option<f64>,
    created_after: Option<String>,
    created_before: Option<String>,
    context_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteRequest {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateRequest {
    id: String,
    content: Option<String>,
    tags: Option<Vec<String>>,
    importance: Option<f64>,
    metadata: Option<serde_json::Value>,
    event_type: Option<String>,
    priority: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RelationsRequest {
    /// Action: "list" (default), "add", "traverse", "version_chain".
    action: Option<String>,
    /// Memory ID (required for list, traverse, version_chain).
    id: Option<String>,
    /// Source memory ID (required for add).
    source_id: Option<String>,
    /// Target memory ID (required for add).
    target_id: Option<String>,
    /// Relationship type (required for add).
    rel_type: Option<String>,
    /// Relationship weight (for add, default 1.0).
    weight: Option<f64>,
    /// Additional metadata (for add).
    metadata: Option<serde_json::Value>,
    /// Max hops for traverse (default 2).
    max_hops: Option<usize>,
    /// Min weight threshold for traverse (default 0.0).
    min_weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FeedbackRequest {
    memory_id: String,
    rating: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LifecycleRequest {
    /// Action: "sweep" (default, TTL expiration), "health", "consolidate", "compact", "auto_compact", "clear_session".
    action: Option<String>,
    warn_mb: Option<f64>,
    critical_mb: Option<f64>,
    max_nodes: Option<i64>,
    prune_days: Option<i64>,
    max_summaries: Option<i64>,
    event_type: Option<String>,
    similarity_threshold: Option<f64>,
    min_cluster_size: Option<usize>,
    dry_run: Option<bool>,
    session_id: Option<String>,
    /// Memory count threshold for auto_compact (default 500).
    count_threshold: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckpointRequest {
    /// Action: "save" (default) or "resume".
    action: Option<String>,
    /// Task title (required for save, optional filter for resume).
    task_title: Option<String>,
    /// Progress description (required for save).
    progress: Option<String>,
    plan: Option<String>,
    files_touched: Option<serde_json::Value>,
    decisions: Option<Vec<String>>,
    key_context: Option<String>,
    next_steps: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
    /// Number of checkpoints to return (for resume, default 1).
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemindRequest {
    action: Option<String>,
    text: Option<String>,
    duration: Option<String>,
    context: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
    status: Option<String>,
    reminder_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LessonsRequest {
    task: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
    exclude_session: Option<String>,
    agent_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AdminRequest {
    /// Action: "health" (default), "export", or "import".
    action: Option<String>,
    /// Detail level for action=health: "basic" (default), "stats", "types", "sessions", "digest", "access_rate".
    detail: Option<String>,
    /// Days for health detail=digest (default 7).
    days: Option<i64>,
    /// JSON data to import for action=import.
    data: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SessionInfoRequest {
    /// Mode: "welcome" (default) or "protocol".
    mode: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProfileRequest {
    action: Option<String>,
    update: Option<serde_json::Value>,
}

// ──────────────────────── Tool implementations ────────────────────────

#[tool_router]
impl McpMemoryServer {
    #[tool(
        name = "memory_store",
        description = "Store memory content in SQLite and return the memory id"
    )]
    async fn memory_store(
        &self,
        params: Parameters<StoreRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (id, input) = build_memory_input(&params.0)?;
        <SqliteStorage as Storage>::store(&self.storage, &id, &params.0.content, &input)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to store memory: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": id }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_store_batch",
        description = "Batch store multiple memories with optimized embedding computation. Pre-warms embedding cache with a single batched inference call for better throughput."
    )]
    async fn memory_store_batch(
        &self,
        params: Parameters<StoreBatchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mut batch_items = Vec::with_capacity(params.0.items.len());

        for item in &params.0.items {
            let (id, input) = build_memory_input(item)?;
            batch_items.push((id, item.content.clone(), input));
        }

        self.storage
            .store_batch(&batch_items)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to batch store: {e}"), None))?;

        let ids: Vec<&str> = batch_items.iter().map(|(id, _, _)| id.as_str()).collect();
        Ok(CallToolResult::success(vec![Content::text(
            json!({ "ids": ids, "count": ids.len() }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_retrieve",
        description = "Retrieve stored memory content by memory id"
    )]
    async fn memory_retrieve(
        &self,
        params: Parameters<RetrieveRequest>,
    ) -> Result<CallToolResult, McpError> {
        let content = self.storage.retrieve(&params.0.id).await.map_err(|e| {
            McpError::internal_error(format!("failed to retrieve memory: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": params.0.id, "content": content }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_search",
        description = "Search stored memories. Modes: 'text' (default, FTS5), 'semantic' (embedding similarity), 'phrase' (exact substring), 'tag' (AND-match tags), 'similar' (find similar to memory_id). advanced=true enables multi-phase retrieval; only 'text' and 'semantic' modes support it ('phrase', 'tag', 'similar' always use their standard paths). Text mode defaults to advanced=true. Required params vary by mode: text/semantic/phrase need 'query', tag needs 'tags', similar needs 'memory_id'."
    )]
    async fn memory_search(
        &self,
        params: Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let mode = params.0.mode.as_deref().unwrap_or("text");
        let limit = params.0.limit.unwrap_or(10);
        let use_advanced = params.0.advanced.unwrap_or(mode == "text");

        // "similar" mode doesn't use opts — early-return path
        if mode == "similar" {
            let memory_id = params.0.memory_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("memory_id is required for mode=similar", None)
            })?;
            let results =
                <SqliteStorage as SimilarFinder>::find_similar(&self.storage, memory_id, limit)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(
                            format!("failed to find similar memories: {e}"),
                            None,
                        )
                    })?;
            let payload = serialize_results(results)?;
            return Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]));
        }

        // All other modes share event_type validation and SearchOptions
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let opts = SearchOptions {
            event_type: EventType::from_optional(params.0.event_type.as_deref()),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            importance_min: params.0.importance_min,
            created_after: params.0.created_after.clone(),
            created_before: params.0.created_before.clone(),
            context_tags: params.0.context_tags.clone(),
            explain: params.0.explain,
            ..Default::default()
        };

        if use_advanced {
            match mode {
                "text" | "semantic" => {
                    let query = params.0.query.as_deref().ok_or_else(|| {
                        McpError::invalid_params(format!("query is required for mode={mode}"), None)
                    })?;
                    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
                        &self.storage,
                        query,
                        limit,
                        &opts,
                    )
                    .await
                    .map_err(|e| {
                        McpError::internal_error(
                            format!("failed to advanced-search memories: {e}"),
                            None,
                        )
                    })?;

                    let abstained = results.is_empty();
                    let result_count = results.len();
                    let confidence: f64 = results
                        .iter()
                        .filter_map(|r| r.metadata.get("_text_overlap").and_then(|v| v.as_f64()))
                        .fold(0.0f64, f64::max);
                    let payload = serialize_results(results)?;

                    let mut response = json!({
                        "results": payload,
                        "result_count": result_count,
                        "abstained": abstained,
                    });
                    if abstained {
                        response["confidence"] = json!(0.0);
                        response["reason"] = json!(format!(
                            "No results met the relevance threshold (text_overlap < {:.2})",
                            crate::memory_core::ABSTENTION_MIN_TEXT
                        ));
                    } else {
                        response["confidence"] = json!(confidence);
                    }

                    return Ok(CallToolResult::success(vec![Content::text(
                        response.to_string(),
                    )]));
                }
                "phrase" | "tag" => {}
                other => {
                    return Err(McpError::invalid_params(
                        format!(
                            "unknown search mode: {other} (expected text|semantic|phrase|tag|similar)"
                        ),
                        None,
                    ));
                }
            }
        }

        match mode {
            "text" => {
                let query = params.0.query.as_deref().ok_or_else(|| {
                    McpError::invalid_params("query is required for mode=text", None)
                })?;
                let results = self
                    .storage
                    .search(query, limit, &opts)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("failed to search memories: {e}"), None)
                    })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload }).to_string(),
                )]))
            }
            "semantic" => {
                let query = params.0.query.as_deref().ok_or_else(|| {
                    McpError::invalid_params("query is required for mode=semantic", None)
                })?;
                let results = self
                    .storage
                    .semantic_search(query, limit, &opts)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(
                            format!("failed to semantic-search memories: {e}"),
                            None,
                        )
                    })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload }).to_string(),
                )]))
            }
            "phrase" => {
                let query = params.0.query.as_deref().ok_or_else(|| {
                    McpError::invalid_params("query is required for mode=phrase", None)
                })?;
                let results = <SqliteStorage as PhraseSearcher>::phrase_search(
                    &self.storage,
                    query,
                    limit,
                    &opts,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to phrase-search memories: {e}"), None)
                })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload }).to_string(),
                )]))
            }
            "tag" => {
                let tags = params.0.tags.as_ref().ok_or_else(|| {
                    McpError::invalid_params("tags is required for mode=tag", None)
                })?;
                if tags.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        json!({ "results": [] }).to_string(),
                    )]));
                }
                let results = self
                    .storage
                    .get_by_tags(tags, limit, &opts)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("failed to search by tags: {e}"), None)
                    })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload }).to_string(),
                )]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown search mode: {other} (expected text|semantic|phrase|tag|similar)"),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_list",
        description = "List stored memories. Sort: 'created' (default, paginated by creation time with offset) or 'recent' (recently accessed)."
    )]
    async fn memory_list(
        &self,
        params: Parameters<ListRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let sort = params.0.sort.as_deref().unwrap_or("created");
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: EventType::from_optional(params.0.event_type.as_deref()),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            importance_min: params.0.importance_min,
            created_after: params.0.created_after.clone(),
            created_before: params.0.created_before.clone(),
            context_tags: params.0.context_tags.clone(),
            ..Default::default()
        };

        match sort {
            "created" => {
                let offset = params.0.offset.unwrap_or(0);
                let result = self.storage.list(offset, limit, &opts).await.map_err(|e| {
                    McpError::internal_error(format!("failed to list memories: {e}"), None)
                })?;
                let payload = serialize_results(result.memories)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload, "total": result.total }).to_string(),
                )]))
            }
            "recent" => {
                let results = self.storage.recent(limit, &opts).await.map_err(|e| {
                    McpError::internal_error(format!("failed to list recents: {e}"), None)
                })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": payload }).to_string(),
                )]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown sort: {other} (expected created|recent)"),
                None,
            )),
        }
    }

    #[tool(name = "memory_delete", description = "Delete a memory by its id")]
    async fn memory_delete(
        &self,
        params: Parameters<DeleteRequest>,
    ) -> Result<CallToolResult, McpError> {
        let deleted =
            self.storage.delete(&params.0.id).await.map_err(|e| {
                McpError::internal_error(format!("failed to delete memory: {e}"), None)
            })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": params.0.id, "deleted": deleted }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_update",
        description = "Update content and optionally tags of an existing memory"
    )]
    async fn memory_update(
        &self,
        params: Parameters<UpdateRequest>,
    ) -> Result<CallToolResult, McpError> {
        if params.0.content.is_none()
            && params.0.tags.is_none()
            && params.0.importance.is_none()
            && params.0.metadata.is_none()
            && params.0.event_type.is_none()
            && params.0.priority.is_none()
        {
            return Err(McpError::invalid_params(
                "at least one of content, tags, importance, metadata, event_type, or priority must be provided",
                None,
            ));
        }
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let update = MemoryUpdate {
            content: params.0.content.clone(),
            tags: params.0.tags.clone(),
            importance: params.0.importance,
            metadata: params.0.metadata.clone(),
            event_type: EventType::from_optional(params.0.event_type.as_deref()),
            priority: params.0.priority,
        };
        <SqliteStorage as Updater>::update(&self.storage, &params.0.id, &update)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to update memory: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": params.0.id, "updated": true }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_relations",
        description = "Manage memory relationships and graph traversal. Actions: 'list' (default, get relationships for a memory), 'add' (create directed relationship), 'traverse' (BFS graph traversal), 'version_chain' (full version history)."
    )]
    async fn memory_relations(
        &self,
        params: Parameters<RelationsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("list");

        match action {
            "list" => {
                let id = params.0.id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("id is required for action=list", None)
                })?;
                let rels = self.storage.get_relationships(id).await.map_err(|e| {
                    McpError::internal_error(format!("failed to get relationships: {e}"), None)
                })?;
                let payload: Vec<_> = rels
                    .into_iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "source_id": r.source_id,
                            "target_id": r.target_id,
                            "rel_type": r.rel_type,
                            "weight": r.weight,
                            "metadata": r.metadata,
                            "created_at": r.created_at
                        })
                    })
                    .collect();
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "relationships": payload }).to_string(),
                )]))
            }
            "add" => {
                let source_id = params.0.source_id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("source_id is required for action=add", None)
                })?;
                let target_id = params.0.target_id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("target_id is required for action=add", None)
                })?;
                let rel_type = params.0.rel_type.as_deref().ok_or_else(|| {
                    McpError::invalid_params("rel_type is required for action=add", None)
                })?;
                let weight = params.0.weight.unwrap_or(1.0);
                if !(0.0..=1.0).contains(&weight) {
                    return Err(McpError::invalid_params(
                        "weight must be between 0.0 and 1.0",
                        None,
                    ));
                }
                let metadata = params
                    .0
                    .metadata
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({}));
                let rel_id = self
                    .storage
                    .add_relationship(source_id, target_id, rel_type, weight, &metadata)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("failed to add relationship: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "id": rel_id, "source_id": source_id, "target_id": target_id, "rel_type": rel_type, "weight": weight, "metadata": metadata }).to_string(),
                )]))
            }
            "traverse" => {
                let id = params.0.id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("id is required for action=traverse", None)
                })?;
                self.storage.retrieve(id).await.map_err(|e| {
                    McpError::internal_error(format!("memory not found for traversal: {e}"), None)
                })?;
                let max_hops = params.0.max_hops.unwrap_or(2);
                if max_hops > 5 {
                    return Err(McpError::invalid_params(
                        "max_hops must be between 1 and 5",
                        None,
                    ));
                }
                let min_weight = params.0.min_weight.unwrap_or(0.0);
                let nodes = <SqliteStorage as GraphTraverser>::traverse(
                    &self.storage,
                    id,
                    max_hops,
                    min_weight,
                    None,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to traverse graph: {e}"), None)
                })?;
                let mut grouped = serde_json::Map::new();
                for node in nodes {
                    let key = node.hop.to_string();
                    let entry = grouped
                        .entry(key)
                        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                    if let serde_json::Value::Array(items) = entry {
                        items.push(json!({
                            "id": node.id,
                            "content": node.content,
                            "event_type": node.event_type,
                            "metadata": node.metadata,
                            "hop": node.hop,
                            "weight": node.weight,
                            "edge_type": node.edge_type,
                            "created_at": node.created_at
                        }));
                    }
                }
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::Value::Object(grouped).to_string(),
                )]))
            }
            "version_chain" => {
                let id = params.0.id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("id is required for action=version_chain", None)
                })?;
                let results = self.storage.get_version_chain(id).await.map_err(|e| {
                    McpError::internal_error(format!("failed to get version chain: {e}"), None)
                })?;
                let payload = serialize_results(results)?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "chain": payload }).to_string(),
                )]))
            }
            other => Err(McpError::invalid_params(
                format!(
                    "unknown relations action: {other} (expected list|add|traverse|version_chain)"
                ),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_feedback",
        description = "Record user feedback signal for a memory"
    )]
    async fn memory_feedback(
        &self,
        params: Parameters<FeedbackRequest>,
    ) -> Result<CallToolResult, McpError> {
        let rating = params.0.rating.as_str();
        if !matches!(rating, "helpful" | "unhelpful" | "outdated") {
            return Err(McpError::invalid_params("invalid rating", None));
        }

        let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
            &self.storage,
            &params.0.memory_id,
            rating,
            params.0.reason.as_deref(),
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to record feedback: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({"memory_id": params.0.memory_id, "feedback": result}).to_string(),
        )]))
    }

    #[tool(
        name = "memory_lifecycle",
        description = "System maintenance. Actions: 'sweep' (default, expire TTL-based memories), 'health' (diagnostic with thresholds), 'consolidate' (prune stale data), 'compact' (merge near-duplicates), 'auto_compact' (embedding-based dedup), 'clear_session' (remove session data)."
    )]
    async fn memory_lifecycle(
        &self,
        params: Parameters<LifecycleRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("sweep");
        let req = &params.0;

        match action {
            "sweep" => {
                let swept_count =
                    <SqliteStorage as ExpirationSweeper>::sweep_expired(&self.storage)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("failed to sweep expired: {e}"), None)
                        })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "swept_count": swept_count }).to_string(),
                )]))
            }
            "health" => {
                let warn = req.warn_mb.unwrap_or(350.0);
                let crit = req.critical_mb.unwrap_or(800.0);
                let max = req.max_nodes.unwrap_or(10000);
                let result = self
                    .storage
                    .check_health(warn, crit, max)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("health check failed: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "consolidate" => {
                let prune = req.prune_days.unwrap_or(30);
                let max_sum = req.max_summaries.unwrap_or(50);
                let result = self
                    .storage
                    .consolidate(prune, max_sum)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("consolidation failed: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "compact" => {
                if let Some(event_type) = req.event_type.as_deref()
                    && !is_valid_event_type(event_type)
                {
                    return Err(McpError::invalid_params("invalid event_type", None));
                }
                let et = req.event_type.as_deref().unwrap_or("lesson_learned");
                let thresh = req.similarity_threshold.unwrap_or(0.6);
                let min_cs = req.min_cluster_size.unwrap_or(3);
                let dry = req.dry_run.unwrap_or(false);
                let result = self
                    .storage
                    .compact(et, thresh, min_cs, dry)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("compaction failed: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "auto_compact" => {
                let threshold = req.count_threshold.unwrap_or(500);
                let dry = req.dry_run.unwrap_or(false);
                let result = self
                    .storage
                    .auto_compact(threshold, dry)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("auto_compact failed: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "clear_session" => {
                let sid = req.session_id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("session_id is required for clear_session", None)
                })?;
                let removed = self.storage.clear_session(sid).await.map_err(|e| {
                    McpError::internal_error(format!("clear_session failed: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({"session_id": sid, "removed": removed}).to_string(),
                )]))
            }
            other => Err(McpError::invalid_params(
                format!(
                    "unknown lifecycle action: {other} (expected sweep|health|consolidate|compact|auto_compact|clear_session)"
                ),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_checkpoint",
        description = "Manage cross-session task checkpoints. Actions: 'save' (default, save a checkpoint) or 'resume' (retrieve prior checkpoints)."
    )]
    async fn memory_checkpoint(
        &self,
        params: Parameters<CheckpointRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("save");

        match action {
            "save" => {
                let task_title = params.0.task_title.as_deref().ok_or_else(|| {
                    McpError::invalid_params("task_title is required for action=save", None)
                })?;
                let progress = params.0.progress.as_deref().ok_or_else(|| {
                    McpError::invalid_params("progress is required for action=save", None)
                })?;
                let input = CheckpointInput {
                    task_title: task_title.to_string(),
                    progress: progress.to_string(),
                    plan: params.0.plan.clone(),
                    files_touched: params.0.files_touched.clone(),
                    decisions: params.0.decisions.clone(),
                    key_context: params.0.key_context.clone(),
                    next_steps: params.0.next_steps.clone(),
                    session_id: params.0.session_id.clone(),
                    project: params.0.project.clone(),
                };
                let memory_id =
                    <SqliteStorage as CheckpointManager>::save_checkpoint(&self.storage, input)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("failed to save checkpoint: {e}"),
                                None,
                            )
                        })?;

                let latest = <SqliteStorage as CheckpointManager>::resume_task(
                    &self.storage,
                    task_title,
                    params.0.project.as_deref(),
                    1,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(
                        format!("failed to resolve checkpoint number: {e}"),
                        None,
                    )
                })?;
                let checkpoint_number = latest
                    .first()
                    .and_then(|entry| entry.get("metadata"))
                    .and_then(|metadata| metadata.get("checkpoint_number"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(1);

                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "memory_id": memory_id, "checkpoint_number": checkpoint_number })
                        .to_string(),
                )]))
            }
            "resume" => {
                let query = params.0.task_title.clone().unwrap_or_default();
                let limit = params.0.limit.unwrap_or(1);
                let results = <SqliteStorage as CheckpointManager>::resume_task(
                    &self.storage,
                    &query,
                    params.0.project.as_deref(),
                    limit,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to resume task: {e}"), None)
                })?;

                let mut markdown = String::new();
                for (index, entry) in results.iter().enumerate() {
                    if index > 0 {
                        markdown.push_str("\n\n---\n\n");
                    }
                    markdown.push_str("### Checkpoint\n");
                    markdown.push_str(entry["content"].as_str().unwrap_or(""));
                    markdown.push_str("\n\nMetadata:\n");
                    markdown.push_str(&entry["metadata"].to_string());
                    markdown.push_str("\n\nCreated At: ");
                    markdown.push_str(entry["created_at"].as_str().unwrap_or(""));
                }

                Ok(CallToolResult::success(vec![Content::text(markdown)]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown checkpoint action: {other} (expected save|resume)"),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_remind",
        description = "Set, list, or dismiss reminders"
    )]
    async fn memory_remind(
        &self,
        params: Parameters<RemindRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("set");
        match action {
            "set" => {
                let text = params.0.text.as_deref().ok_or_else(|| {
                    McpError::invalid_params("text is required for action=set", None)
                })?;
                let duration = params.0.duration.as_deref().ok_or_else(|| {
                    McpError::invalid_params("duration is required for action=set", None)
                })?;
                let result = <SqliteStorage as ReminderManager>::create_reminder(
                    &self.storage,
                    text,
                    duration,
                    params.0.context.as_deref(),
                    params.0.session_id.as_deref(),
                    params.0.project.as_deref(),
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to create reminder: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "list" => {
                let result = <SqliteStorage as ReminderManager>::list_reminders(
                    &self.storage,
                    params.0.status.as_deref(),
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to list reminders: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": result }).to_string(),
                )]))
            }
            "dismiss" => {
                let reminder_id = params.0.reminder_id.as_deref().ok_or_else(|| {
                    McpError::invalid_params("reminder_id is required for action=dismiss", None)
                })?;
                let result = <SqliteStorage as ReminderManager>::dismiss_reminder(
                    &self.storage,
                    reminder_id,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to dismiss reminder: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            _ => Err(McpError::invalid_params(
                "action must be one of: set, list, dismiss",
                None,
            )),
        }
    }

    #[tool(
        name = "memory_lessons",
        description = "Query lesson_learned memories for a task or project"
    )]
    async fn memory_lessons(
        &self,
        params: Parameters<LessonsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.0.limit.unwrap_or(5);
        let lessons = <SqliteStorage as LessonQuerier>::query_lessons(
            &self.storage,
            params.0.task.as_deref(),
            params.0.project.as_deref(),
            params.0.exclude_session.as_deref(),
            params.0.agent_type.as_deref(),
            limit,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to query lessons: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": lessons }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_admin",
        description = "Administrative actions. action='health' (default, detail: basic|stats|types|sessions|digest|access_rate), action='export' (dump all data), or action='import' (restore from JSON data)."
    )]
    async fn memory_admin(
        &self,
        params: Parameters<AdminRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("health");

        match action {
            "health" => {
                let detail = params.0.detail.as_deref().unwrap_or("basic");
                match detail {
                    "basic" => {
                        self.storage.stats().await.map_err(|e| {
                            McpError::internal_error(format!("storage probe failed: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            json!({ "status": "healthy" }).to_string(),
                        )]))
                    }
                    "stats" => {
                        let stats = self.storage.stats().await.map_err(|e| {
                            McpError::internal_error(format!("failed to get stats: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string(&stats).map_err(|e| {
                                McpError::internal_error(
                                    format!("failed to serialize stats: {e}"),
                                    None,
                                )
                            })?,
                        )]))
                    }
                    "types" => {
                        let result = self.storage.type_stats().await.map_err(|e| {
                            McpError::internal_error(format!("type_stats failed: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            result.to_string(),
                        )]))
                    }
                    "sessions" => {
                        let result = self.storage.session_stats().await.map_err(|e| {
                            McpError::internal_error(format!("session_stats failed: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            result.to_string(),
                        )]))
                    }
                    "digest" => {
                        let days = params.0.days.unwrap_or(7);
                        let result = self.storage.weekly_digest(days).await.map_err(|e| {
                            McpError::internal_error(format!("weekly_digest failed: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            result.to_string(),
                        )]))
                    }
                    "access_rate" => {
                        let result = self.storage.access_rate_stats().await.map_err(|e| {
                            McpError::internal_error(format!("access_rate_stats failed: {e}"), None)
                        })?;
                        Ok(CallToolResult::success(vec![Content::text(
                            result.to_string(),
                        )]))
                    }
                    other => Err(McpError::invalid_params(
                        format!(
                            "unknown detail level: {other} (expected basic|stats|types|sessions|digest|access_rate)"
                        ),
                        None,
                    )),
                }
            }
            "export" => {
                let export_data = self.storage.export_all().await.map_err(|e| {
                    McpError::internal_error(format!("failed to export: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(export_data)]))
            }
            "import" => {
                let data = params.0.data.as_deref().ok_or_else(|| {
                    McpError::invalid_params("data is required for action=import", None)
                })?;
                let count = self.storage.import_all(data).await.map_err(|e| {
                    McpError::internal_error(format!("failed to import: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "imported_memories": count.0, "imported_relationships": count.1 })
                        .to_string(),
                )]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown admin action: {other} (expected health|export|import)"),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_profile",
        description = "Read or update the cross-session user profile"
    )]
    async fn memory_profile(
        &self,
        params: Parameters<ProfileRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_deref().unwrap_or("read");
        match action {
            "read" => {
                let profile = <SqliteStorage as ProfileManager>::get_profile(&self.storage)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("failed to read profile: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    profile.to_string(),
                )]))
            }
            "update" => {
                let updates = params.0.update.as_ref().ok_or_else(|| {
                    McpError::invalid_params("update payload is required for action=update", None)
                })?;
                <SqliteStorage as ProfileManager>::set_profile(&self.storage, updates)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("failed to update profile: {e}"), None)
                    })?;
                Ok(CallToolResult::success(vec![Content::text(
                    json!({ "updated": true }).to_string(),
                )]))
            }
            _ => Err(McpError::invalid_params(
                "action must be one of: read, update",
                None,
            )),
        }
    }

    #[tool(
        name = "memory_session_info",
        description = "Session-oriented information. mode='welcome' (default) returns the startup briefing; mode='protocol' returns the tool inventory and usage guidelines."
    )]
    async fn memory_session_info(
        &self,
        params: Parameters<SessionInfoRequest>,
    ) -> Result<CallToolResult, McpError> {
        match params.0.mode.as_deref().unwrap_or("welcome") {
            "welcome" => {
                let result = self
                    .storage
                    .welcome(params.0.session_id.as_deref(), params.0.project.as_deref())
                    .await
                    .map_err(|e| McpError::internal_error(format!("welcome failed: {e}"), None))?;

                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "protocol" => {
                let protocol = r#"# MAG Protocol

## Available Tools (16)

### Storage & Retrieval
- **memory_store** — Store new memory content with tags, importance, metadata
- **memory_store_batch** — Batch store multiple memories with optimized embedding
- **memory_retrieve** — Retrieve a memory by ID
- **memory_delete** — Delete a memory by ID
- **memory_update** — Update content, tags, importance, or metadata

### Search & Listing
- **memory_search** — Unified search (mode: text|semantic|phrase|tag|similar, advanced: bool)
- **memory_list** — List memories (sort: created|recent)

### Relationships & Graph
- **memory_relations** — Manage relationships (action: list|add|traverse|version_chain)

### Lifecycle & Feedback
- **memory_feedback** — Record feedback (helpful/unhelpful/outdated)
- **memory_lifecycle** — System maintenance (action: sweep|health|consolidate|compact|auto_compact|clear_session)

### Cross-Session
- **memory_checkpoint** — Task checkpoints (action: save|resume)
- **memory_remind** — Set, list, or dismiss reminders
- **memory_lessons** — Query lesson_learned memories
- **memory_profile** — Read/update user profile

### System
- **memory_admin** — Administrative actions (action: health|export|import)
- **memory_session_info** — Welcome briefing or protocol (mode: welcome|protocol)

## Usage Guidelines
- Call **memory_session_info** with `mode="welcome"` at session start for context
- Use **memory_search** with `advanced=true` when you want the multi-phase retrieval path for a supported mode
- Use **memory_lifecycle** with action=sweep periodically to clean expired memories
- Use **memory_lifecycle** with action=consolidate to prune stale data
"#;
                Ok(CallToolResult::success(vec![Content::text(protocol)]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown session info mode: {other} (expected welcome|protocol)"),
                None,
            )),
        }
    }
}

#[tool_handler]
impl ServerHandler for McpMemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "MAG -- local memory server for AI agents.\n\n\
                 STARTUP: Call memory_session_info(mode=\"welcome\") first to get session context.\n\
                 REFERENCE: Call memory_session_info(mode=\"protocol\") for tool inventory.\n\
                 DIAGNOSTICS: If issues occur, run CLI: mag doctor"
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
