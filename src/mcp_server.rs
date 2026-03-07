use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use std::str::FromStr;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, EventType, ExpirationSweeper,
    FeedbackRecorder, GraphTraverser, LessonQuerier, Lister, MaintenanceManager, MemoryInput,
    MemoryUpdate, PhraseSearcher, ProfileManager, Recents, RelationshipQuerier, ReminderManager,
    Retriever, SearchOptions, Searcher, SemanticSearcher, SimilarFinder, StatsProvider, Storage,
    Tagger, Updater, VersionChainQuerier, WelcomeProvider, default_priority_for_event_type,
    default_ttl_for_event_type, is_valid_event_type,
};

/// Converts an `Option<String>` (from JSON request) into `Option<EventType>`.
fn parse_event_type(s: &Option<String>) -> Option<EventType> {
    s.as_ref()
        .map(|v| EventType::from_str(v).unwrap_or_else(|e| match e {}))
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
struct RetrieveRequest {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchRequest {
    query: String,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SemanticSearchRequest {
    query: String,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AdvancedSearchRequest {
    query: String,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    importance_min: Option<f64>,
    created_after: Option<String>,
    created_before: Option<String>,
    context_tags: Option<Vec<String>>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct VersionChainRequest {
    memory_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SimilarRequest {
    memory_id: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TraverseRequest {
    memory_id: String,
    max_hops: Option<usize>,
    min_weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PhraseSearchRequest {
    phrase: String,
    limit: Option<usize>,
    event_type: Option<String>,
    include_superseded: Option<bool>,
    project: Option<String>,
    session_id: Option<String>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecentRequest {
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
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
struct StatsRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExportRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
struct ImportRequest {
    data: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MaintainRequest {
    action: String,
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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WelcomeRequest {
    session_id: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProtocolRequest {
    #[allow(dead_code)]
    section: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatsExtendedRequest {
    action: String,
    days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TagSearchRequest {
    tags: Vec<String>,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    include_superseded: Option<bool>,
    /// ISO 8601 lower bound for event_at (inclusive).
    event_after: Option<String>,
    /// ISO 8601 upper bound for event_at (inclusive).
    event_before: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListRequest {
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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RelationsRequest {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddRelationRequest {
    source_id: String,
    target_id: String,
    rel_type: String,
    weight: Option<f64>,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FeedbackRequest {
    memory_id: String,
    rating: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SweepRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProfileRequest {
    action: Option<String>,
    update: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckpointRequest {
    task_title: String,
    progress: String,
    plan: Option<String>,
    files_touched: Option<serde_json::Value>,
    decisions: Option<Vec<String>>,
    key_context: Option<String>,
    next_steps: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ResumeTaskRequest {
    task_title: Option<String>,
    project: Option<String>,
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
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }

        let id = params
            .0
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let event_type_str = params.0.event_type.clone();
        let ttl_seconds = params.0.ttl_seconds.or_else(|| {
            event_type_str
                .as_deref()
                .map(default_ttl_for_event_type)
                .unwrap_or(Some(crate::memory_core::TTL_LONG_TERM))
        });
        let input = MemoryInput {
            content: params.0.content.clone(),
            id: Some(id.clone()),
            tags: params.0.tags.clone().unwrap_or_default(),
            importance: params.0.importance.unwrap_or(0.5),
            metadata: params
                .0
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            priority: params.0.priority.or_else(|| {
                event_type_str
                    .as_deref()
                    .map(default_priority_for_event_type)
            }),
            event_type: parse_event_type(&event_type_str),
            session_id: params.0.session_id.clone(),
            project: params.0.project.clone(),
            entity_id: params.0.entity_id.clone(),
            agent_type: params.0.agent_type.clone(),
            ttl_seconds,
            referenced_date: params.0.referenced_date.clone(),
        };
        <SqliteStorage as Storage>::store(&self.storage, &id, &params.0.content, &input)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to store memory: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": id }).to_string(),
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
        description = "Search stored memories by query string"
    )]
    async fn memory_search(
        &self,
        params: Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results = self
            .storage
            .search(&params.0.query, limit, &opts)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to search memories: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_semantic_search",
        description = "Perform semantic search over stored memories"
    )]
    async fn memory_semantic_search(
        &self,
        params: Parameters<SemanticSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results = self
            .storage
            .semantic_search(&params.0.query, limit, &opts)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to semantic-search memories: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "score": r.score,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_advanced_search",
        description = "Perform advanced multi-phase search with scoring and filters"
    )]
    async fn memory_advanced_search(
        &self,
        params: Parameters<AdvancedSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }

        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            importance_min: params.0.importance_min,
            created_after: params.0.created_after.clone(),
            created_before: params.0.created_before.clone(),
            context_tags: params.0.context_tags.clone(),
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &self.storage,
            &params.0.query,
            limit,
            &opts,
        )
        .await
        .map_err(|e| {
            McpError::internal_error(format!("failed to advanced-search memories: {e}"), None)
        })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "score": r.score,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_similar",
        description = "Find memories similar to a source memory by embedding"
    )]
    async fn memory_similar(
        &self,
        params: Parameters<SimilarRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.storage
            .retrieve(&params.0.memory_id)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("memory not found for similar search: {e}"), None)
            })?;

        let limit = params.0.limit.unwrap_or(5);
        let results = <SqliteStorage as SimilarFinder>::find_similar(
            &self.storage,
            &params.0.memory_id,
            limit,
        )
        .await
        .map_err(|e| {
            McpError::internal_error(format!("failed to find similar memories: {e}"), None)
        })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "score": r.score,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_version_chain",
        description = "Get the full version history chain for a memory"
    )]
    async fn memory_version_chain(
        &self,
        params: Parameters<VersionChainRequest>,
    ) -> Result<CallToolResult, McpError> {
        let results = self
            .storage
            .get_version_chain(&params.0.memory_id)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to get version chain: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "chain": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_traverse",
        description = "Traverse related memories using graph relationships"
    )]
    async fn memory_traverse(
        &self,
        params: Parameters<TraverseRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.storage
            .retrieve(&params.0.memory_id)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("memory not found for traversal: {e}"), None)
            })?;

        let max_hops = params.0.max_hops.unwrap_or(2);
        let min_weight = params.0.min_weight.unwrap_or(0.0);
        let nodes = <SqliteStorage as GraphTraverser>::traverse(
            &self.storage,
            &params.0.memory_id,
            max_hops,
            min_weight,
            None,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to traverse graph: {e}"), None))?;

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

    #[tool(
        name = "memory_phrase_search",
        description = "Search memories by exact phrase substring"
    )]
    async fn memory_phrase_search(
        &self,
        params: Parameters<PhraseSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }

        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results = <SqliteStorage as PhraseSearcher>::phrase_search(
            &self.storage,
            &params.0.phrase,
            limit,
            &opts,
        )
        .await
        .map_err(|e| {
            McpError::internal_error(format!("failed to phrase-search memories: {e}"), None)
        })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_recent",
        description = "List recently accessed memories"
    )]
    async fn memory_recent(
        &self,
        params: Parameters<RecentRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results =
            self.storage.recent(limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to list recents: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
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
        name = "memory_checkpoint",
        description = "Save a cross-session checkpoint for a task"
    )]
    async fn memory_checkpoint(
        &self,
        params: Parameters<CheckpointRequest>,
    ) -> Result<CallToolResult, McpError> {
        let input = CheckpointInput {
            task_title: params.0.task_title.clone(),
            progress: params.0.progress.clone(),
            plan: params.0.plan.clone(),
            files_touched: params.0.files_touched.clone(),
            decisions: params.0.decisions.clone(),
            key_context: params.0.key_context.clone(),
            next_steps: params.0.next_steps.clone(),
            session_id: params.0.session_id.clone(),
            project: params.0.project.clone(),
        };
        let memory_id = <SqliteStorage as CheckpointManager>::save_checkpoint(&self.storage, input)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to save checkpoint: {e}"), None)
            })?;

        let latest = <SqliteStorage as CheckpointManager>::resume_task(
            &self.storage,
            &params.0.task_title,
            params.0.project.as_deref(),
            1,
        )
        .await
        .map_err(|e| {
            McpError::internal_error(format!("failed to resolve checkpoint number: {e}"), None)
        })?;
        let checkpoint_number = latest
            .first()
            .and_then(|entry| entry.get("metadata"))
            .and_then(|metadata| metadata.get("checkpoint_number"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(1);

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "memory_id": memory_id, "checkpoint_number": checkpoint_number }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_resume_task",
        description = "Resume prior checkpoints for a task"
    )]
    async fn memory_resume_task(
        &self,
        params: Parameters<ResumeTaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let query = params.0.task_title.clone().unwrap_or_default();
        let limit = params.0.limit.unwrap_or(1);
        let results = <SqliteStorage as CheckpointManager>::resume_task(
            &self.storage,
            &query,
            params.0.project.as_deref(),
            limit,
        )
        .await
        .map_err(|e| McpError::internal_error(format!("failed to resume task: {e}"), None))?;

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
        name = "memory_health",
        description = "Return service health information"
    )]
    async fn memory_health(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            "romega-memory MCP server is healthy",
        )]))
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
            event_type: parse_event_type(&params.0.event_type),
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
        name = "memory_tag_search",
        description = "Search memories by tags (AND logic — all tags must match)"
    )]
    async fn memory_tag_search(
        &self,
        params: Parameters<TagSearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        if params.0.tags.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": [] }).to_string(),
            )]));
        }
        if let Some(event_type) = params.0.event_type.as_deref()
            && !is_valid_event_type(event_type)
        {
            return Err(McpError::invalid_params("invalid event_type", None));
        }
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let results = self
            .storage
            .get_by_tags(&params.0.tags, limit, &opts)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to search by tags: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "content": r.content,
                    "tags": r.tags,
                    "importance": r.importance,
                    "metadata": r.metadata,
                    "event_type": r.event_type,
                    "session_id": r.session_id,
                    "project": r.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_list",
        description = "List stored memories with pagination, returning page and total count"
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
        let offset = params.0.offset.unwrap_or(0);
        let limit = params.0.limit.unwrap_or(10);
        let opts = SearchOptions {
            event_type: parse_event_type(&params.0.event_type),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            include_superseded: params.0.include_superseded,
            event_after: params.0.event_after.clone(),
            event_before: params.0.event_before.clone(),
            ..Default::default()
        };
        let result =
            self.storage.list(offset, limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to list memories: {e}"), None)
            })?;

        let payload: Vec<_> = result
            .memories
            .into_iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "content": m.content,
                    "tags": m.tags,
                    "importance": m.importance,
                    "metadata": m.metadata,
                    "event_type": m.event_type,
                    "session_id": m.session_id,
                    "project": m.project
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload, "total": result.total }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_relations",
        description = "Get relationships for a memory (both directions)"
    )]
    async fn memory_relations(
        &self,
        params: Parameters<RelationsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let rels = self
            .storage
            .get_relationships(&params.0.id)
            .await
            .map_err(|e| {
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

    #[tool(
        name = "memory_add_relation",
        description = "Create a directed relationship between two memories"
    )]
    async fn memory_add_relation(
        &self,
        params: Parameters<AddRelationRequest>,
    ) -> Result<CallToolResult, McpError> {
        let rel_id = self
            .storage
            .add_relationship(
                &params.0.source_id,
                &params.0.target_id,
                &params.0.rel_type,
                params.0.weight.unwrap_or(1.0),
                &params
                    .0
                    .metadata
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({})),
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to add relationship: {e}"), None)
            })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": rel_id, "source_id": params.0.source_id, "target_id": params.0.target_id, "rel_type": params.0.rel_type, "weight": params.0.weight.unwrap_or(1.0), "metadata": params.0.metadata.clone().unwrap_or_else(|| serde_json::json!({})) }).to_string(),
        )]))
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
        name = "memory_sweep",
        description = "Sweep expired memories based on TTL"
    )]
    async fn memory_sweep(
        &self,
        #[allow(unused_variables)] params: Parameters<SweepRequest>,
    ) -> Result<CallToolResult, McpError> {
        let swept_count = <SqliteStorage as ExpirationSweeper>::sweep_expired(&self.storage)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to sweep expired: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "swept_count": swept_count }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_stats",
        description = "Return memory store statistics including counts and storage info"
    )]
    async fn memory_stats(
        &self,
        #[allow(unused_variables)] params: Parameters<StatsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let stats = self
            .storage
            .stats()
            .await
            .map_err(|e| McpError::internal_error(format!("failed to get stats: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&stats).map_err(|e| {
                McpError::internal_error(format!("failed to serialize stats: {e}"), None)
            })?,
        )]))
    }

    #[tool(
        name = "memory_export",
        description = "Export all memories and relationships as JSON"
    )]
    async fn memory_export(
        &self,
        #[allow(unused_variables)] params: Parameters<ExportRequest>,
    ) -> Result<CallToolResult, McpError> {
        let export_data = self
            .storage
            .export_all()
            .await
            .map_err(|e| McpError::internal_error(format!("failed to export: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(export_data)]))
    }

    #[tool(
        name = "memory_import",
        description = "Import memories and relationships from JSON"
    )]
    async fn memory_import(
        &self,
        params: Parameters<ImportRequest>,
    ) -> Result<CallToolResult, McpError> {
        let count = self
            .storage
            .import_all(&params.0.data)
            .await
            .map_err(|e| McpError::internal_error(format!("failed to import: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "imported_memories": count.0, "imported_relationships": count.1 }).to_string(),
        )]))
    }

    #[tool(
        name = "memory_maintain",
        description = "System housekeeping: health check, consolidate stale memories, compact near-duplicates, or clear a session"
    )]
    async fn memory_maintain(
        &self,
        params: Parameters<MaintainRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = &params.0;
        let action = req.action.as_str();

        match action {
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
            "backup" => Ok(CallToolResult::success(vec![Content::text(
                "Use the memory_export tool to export data as JSON.".to_string(),
            )])),
            "restore" => Ok(CallToolResult::success(vec![Content::text(
                "Use the memory_import tool to import data from JSON.".to_string(),
            )])),
            other => Err(McpError::invalid_params(
                format!(
                    "unknown maintain action: {other} (expected health|consolidate|compact|clear_session|backup|restore)"
                ),
                None,
            )),
        }
    }

    #[tool(
        name = "memory_welcome",
        description = "Session startup briefing with recent activity, user profile, and pending reminders"
    )]
    async fn memory_welcome(
        &self,
        params: Parameters<WelcomeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .storage
            .welcome(params.0.session_id.as_deref(), params.0.project.as_deref())
            .await
            .map_err(|e| McpError::internal_error(format!("welcome failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(
        name = "memory_protocol",
        description = "Retrieve available tools and operational guidelines for this memory server"
    )]
    async fn memory_protocol(
        &self,
        _params: Parameters<ProtocolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let protocol = r#"# romega-memory Protocol

## Available Tools (31)

### Storage & Retrieval
- **memory_store** — Store new memory content with tags, importance, metadata
- **memory_retrieve** — Retrieve a memory by ID
- **memory_delete** — Delete a memory by ID
- **memory_update** — Update content, tags, importance, or metadata
- **memory_export** — Export all data as JSON
- **memory_import** — Import data from JSON

### Search
- **memory_search** — Full-text search with FTS5
- **memory_semantic_search** — Semantic search via embeddings
- **memory_advanced_search** — Multi-phase scoring (vector + FTS5 + type weights + time decay)
- **memory_similar** — Find similar memories by embedding
- **memory_phrase_search** — Exact phrase search
- **memory_tag_search** — Search by tags
- **memory_list** — Paginated listing with filters
- **memory_recent** — Recently accessed memories

### Relationships & Graph
- **memory_relations** — Get relationships for a memory
- **memory_add_relation** — Create a directed relationship
- **memory_version_chain** — Retrieve full version chain for a memory
- **memory_traverse** — Graph traversal via BFS

### Lifecycle
- **memory_feedback** — Record feedback (helpful/unhelpful/outdated)
- **memory_sweep** — Expire memories by TTL

### Cross-Session
- **memory_profile** — Read/update user profile
- **memory_checkpoint** — Save task checkpoint
- **memory_resume_task** — Resume from prior checkpoints
- **memory_remind** — Set, list, or dismiss reminders
- **memory_lessons** — Query lesson_learned memories

### Maintenance & Stats
- **memory_health** — Basic health check
- **memory_stats** — Basic store statistics
- **memory_maintain** — System housekeeping (health/consolidate/compact/clear_session)
- **memory_welcome** — Session startup briefing
- **memory_protocol** — This tool: available tools and guidelines
- **memory_stats_extended** — Extended analytics (types/sessions/digest/access_rate)

## Usage Guidelines
- Call **memory_welcome** at session start for context
- Use **memory_store** with appropriate event_type and tags for categorization
- Use **memory_sweep** periodically to clean expired memories
- Use **memory_maintain** with action=consolidate to prune stale data
"#;
        Ok(CallToolResult::success(vec![Content::text(protocol)]))
    }

    #[tool(
        name = "memory_stats_extended",
        description = "Extended analytics: per-type counts, session stats, weekly digest, or access rate analysis"
    )]
    async fn memory_stats_extended(
        &self,
        params: Parameters<StatsExtendedRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = params.0.action.as_str();

        match action {
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
                    "unknown stats action: {other} (expected types|sessions|digest|access_rate)"
                ),
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
                "romega-memory MCP server exposes tools for storing and retrieving memories"
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
