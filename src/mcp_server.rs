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

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    AdvancedSearcher, Deleter, ExpirationSweeper, FeedbackRecorder, GraphTraverser, Lister,
    MemoryInput, MemoryUpdate, PhraseSearcher, Recents, RelationshipQuerier, Retriever,
    SearchOptions, Searcher, SemanticSearcher, SimilarFinder, Storage, Tagger, Updater,
    default_priority_for_event_type, default_ttl_for_event_type, is_valid_event_type,
};

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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SemanticSearchRequest {
    query: String,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AdvancedSearchRequest {
    query: String,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
    importance_min: Option<f64>,
    created_after: Option<String>,
    created_before: Option<String>,
    context_tags: Option<Vec<String>>,
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
    project: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecentRequest {
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
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
struct TagSearchRequest {
    tags: Vec<String>,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListRequest {
    offset: Option<usize>,
    limit: Option<usize>,
    event_type: Option<String>,
    project: Option<String>,
    session_id: Option<String>,
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
        let event_type = params.0.event_type.clone();
        let ttl_seconds = params.0.ttl_seconds.or_else(|| {
            event_type
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
            priority: params
                .0
                .priority
                .or_else(|| event_type.as_deref().map(default_priority_for_event_type)),
            event_type,
            session_id: params.0.session_id.clone(),
            project: params.0.project.clone(),
            entity_id: params.0.entity_id.clone(),
            agent_type: params.0.agent_type.clone(),
            ttl_seconds,
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: params.0.importance_min,
            created_after: params.0.created_after.clone(),
            created_before: params.0.created_before.clone(),
            context_tags: params.0.context_tags.clone(),
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
            event_type: params.0.event_type.clone(),
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
            event_type: params.0.event_type.clone(),
            project: params.0.project.clone(),
            session_id: params.0.session_id.clone(),
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
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
