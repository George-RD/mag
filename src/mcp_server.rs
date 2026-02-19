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
use crate::memory_core::{Retriever, Storage};

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
