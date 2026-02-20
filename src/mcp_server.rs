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
    Deleter, Lister, Recents, RelationshipQuerier, Retriever, Searcher, SemanticSearcher, Storage,
    Tagger, Updater,
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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RetrieveRequest {
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchRequest {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SemanticSearchRequest {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecentRequest {
    limit: Option<usize>,
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
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TagSearchRequest {
    tags: Vec<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListRequest {
    offset: Option<usize>,
    limit: Option<usize>,
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
        let id = params
            .0
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let tags = params.0.tags.unwrap_or_default();
        self.storage
            .store(&id, &params.0.content, &tags)
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
        let limit = params.0.limit.unwrap_or(10);
        let results = self
            .storage
            .search(&params.0.query, limit)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to search memories: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| json!({ "id": r.id, "content": r.content }))
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
        let limit = params.0.limit.unwrap_or(10);
        let results = self
            .storage
            .semantic_search(&params.0.query, limit)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to semantic-search memories: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| json!({ "id": r.id, "content": r.content, "score": r.score }))
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
        let limit = params.0.limit.unwrap_or(10);
        let results =
            self.storage.recent(limit).await.map_err(|e| {
                McpError::internal_error(format!("failed to list recents: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| json!({ "id": r.id, "content": r.content }))
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
        if params.0.content.is_none() && params.0.tags.is_none() {
            return Err(McpError::invalid_params(
                "at least one of content or tags must be provided",
                None,
            ));
        }
        let tag_slice = params.0.tags.as_deref();
        self.storage
            .update(&params.0.id, params.0.content.as_deref(), tag_slice)
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
        let limit = params.0.limit.unwrap_or(10);
        let results = self
            .storage
            .get_by_tags(&params.0.tags, limit)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to search by tags: {e}"), None)
            })?;

        let payload: Vec<_> = results
            .into_iter()
            .map(|r| json!({ "id": r.id, "content": r.content }))
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
        let offset = params.0.offset.unwrap_or(0);
        let limit = params.0.limit.unwrap_or(10);
        let result =
            self.storage.list(offset, limit).await.map_err(|e| {
                McpError::internal_error(format!("failed to list memories: {e}"), None)
            })?;

        let payload: Vec<_> = result
            .memories
            .into_iter()
            .map(|m| json!({ "id": m.id, "content": m.content }))
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
                    "rel_type": r.rel_type
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
            .add_relationship(&params.0.source_id, &params.0.target_id, &params.0.rel_type)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to add relationship: {e}"), None)
            })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({ "id": rel_id, "source_id": params.0.source_id, "target_id": params.0.target_id, "rel_type": params.0.rel_type }).to_string(),
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
