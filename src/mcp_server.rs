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
use crate::memory_core::{Recents, Retriever, Searcher, SemanticSearcher, Storage};

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
        self.storage
            .store(&id, &params.0.content)
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
