use std::{fmt::Write as _, sync::Arc};

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{tool::ToolCallContext, tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    tool, tool_router,
    transport::stdio,
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::LocalMemoryRuntime;
#[cfg(test)]
use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{MemoryInput, is_valid_event_type};

mod request_types;
mod tools;
pub(crate) mod validation;

use request_types::*;
use validation::require_finite;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum McpToolMode {
    #[default]
    Full,
    Minimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpToolSurface {
    Legacy,
    Facade,
}

pub struct ToolMeta {
    pub name: &'static str,
    pub summary: &'static str,
    pub category: &'static str,
    pub surface: McpToolSurface,
}

pub const TOOL_REGISTRY: &[ToolMeta] = &[
    ToolMeta {
        name: "memory",
        summary: "Unified facade: store/store_batch/retrieve/delete via action field",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_store",
        summary: "Store new memory content with tags, importance, metadata",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_store_batch",
        summary: "Batch store multiple memories with optimized embedding",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_retrieve",
        summary: "Retrieve a memory by ID",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_delete",
        summary: "Delete a memory by ID",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_update",
        summary: "Update content, tags, importance, or metadata",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_search",
        summary: "Search memories using text, semantic, phrase, tag, or similarity modes",
        category: "Search & Listing",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_list",
        summary: "List memories sorted by creation or recency",
        category: "Search & Listing",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_relations",
        summary: "Manage relationships and graph traversal",
        category: "Relationships & Graph",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_feedback",
        summary: "Record helpful, unhelpful, or outdated feedback",
        category: "Lifecycle & Feedback",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_lifecycle",
        summary: "Run maintenance, health, consolidation, backup, and cleanup actions",
        category: "Lifecycle & Feedback",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_checkpoint",
        summary: "Save or resume task checkpoints",
        category: "Cross-Session",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_remind",
        summary: "Set, list, or dismiss reminders",
        category: "Cross-Session",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_lessons",
        summary: "Query lesson-learned memories",
        category: "Cross-Session",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_profile",
        summary: "Read or update the user profile",
        category: "Cross-Session",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_session_info",
        summary: "Return the welcome briefing or MCP protocol information",
        category: "System",
        surface: McpToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_manage",
        summary: "Unified facade: update/feedback/relations/lifecycle via action field",
        category: "Storage & Retrieval",
        surface: McpToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_session",
        summary: "Unified facade: info/checkpoint/remind/lessons/profile via action field",
        category: "Cross-Session",
        surface: McpToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_admin",
        summary: "Unified facade: health/list/export/import via action field",
        category: "System",
        surface: McpToolSurface::Facade,
    },
];

const CATEGORY_ORDER: &[&str] = &[
    "Storage & Retrieval",
    "Search & Listing",
    "Relationships & Graph",
    "Lifecycle & Feedback",
    "Cross-Session",
    "System",
];

fn tools_for_surface(surface: McpToolSurface) -> impl Iterator<Item = &'static ToolMeta> {
    TOOL_REGISTRY.iter().filter(move |tool| tool.surface == surface)
}

fn facade_tool_names() -> Vec<&'static str> {
    tools_for_surface(McpToolSurface::Facade)
        .map(|tool| tool.name)
        .collect()
}

fn legacy_tool_count() -> usize {
    tools_for_surface(McpToolSurface::Legacy).count()
}

fn mcp_instructions() -> String {
    let facade_names = facade_tool_names();
    format!(
        "MAG — optional MCP transport for MAG's canonical CLI/runtime workflows.\n\n\
## SESSION START\n\
Call memory_session(action=\"info\", info_mode=\"welcome\") before substantive work when MCP is your only MAG integration.\n\n\
## STORE DECISIONS AND LESSONS\n\
After non-trivial work, persist architectural decisions, bug root causes, user preferences, corrections, and reusable workflow lessons.\n\n\
## SEARCH BEFORE DECIDING\n\
Check prior memory before recurring debugging or architecture decisions.\n\n\
## DO NOT ANNOUNCE MEMORY OPERATIONS\n\
Use memory operations silently and weave recalled context into normal responses.\n\n\
## TOOL REFERENCE\n\
Call memory_session(action=\"info\", info_mode=\"protocol\") for the generated tool inventory.\n\n\
## DIAGNOSTICS\n\
If MCP calls fail, ask the user to run: mag doctor\n\n\
## CANONICAL INTERFACE\n\
Prefer MAG CLI commands and hooks when the host can run them. MCP is a thin optional transport over the same runtime behavior.\n\n\
## UNIFIED MCP INTERFACE\n\
The {} facade tools are: {}. Full mode also exposes {} legacy compatibility tools. Use --mcp-tools=minimal to advertise only the facade tools.\n",
        facade_names.len(),
        facade_names.join(", "),
        legacy_tool_count(),
    )
}

pub fn generate_protocol_markdown() -> String {
    let count = TOOL_REGISTRY.len();
    let mut out = format!("# MAG Protocol\n\n## Available Tools ({count})\n");

    for &cat in CATEGORY_ORDER {
        let _ = write!(out, "\n### {cat}\n");
        for tool in TOOL_REGISTRY.iter().filter(|t| t.category == cat) {
            let _ = writeln!(out, "- **{}** — {}", tool.name, tool.summary);
        }
    }

    let facade_names = facade_tool_names();
    let _ = write!(
        out,
        "\n## Interface Modes\n- Minimal: {} facade tools ({})\n- Full: {} total tools ({} facade + {} legacy compatibility)\n",
        facade_names.len(),
        facade_names.join(", "),
        TOOL_REGISTRY.len(),
        facade_names.len(),
        legacy_tool_count(),
    );

    out.push_str(
        "\n## Usage Guidelines\n\
         - Prefer the MAG CLI when the host can execute it; MCP is an optional transport\n\
         - Call **memory_session** with `action=\"info\"` and `info_mode=\"welcome\"` at session start in MCP-only hosts\n\
         - Prefer the four facade tools for new MCP integrations\n\
         - Keep legacy tools for compatibility only\n",
    );

    out
}

pub fn tool_registry_json() -> serde_json::Value {
    let names: Vec<&str> = TOOL_REGISTRY.iter().map(|t| t.name).collect();
    let facade_names = facade_tool_names();
    json!({
        "tools": names,
        "tool_count": TOOL_REGISTRY.len(),
        "facade_tools": facade_names,
        "facade_tool_count": tools_for_surface(McpToolSurface::Facade).count(),
        "legacy_tool_count": legacy_tool_count(),
    })
}

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

fn build_memory_input(item: &StoreRequest) -> Result<(String, MemoryInput), McpError> {
    if let Some(et) = item.event_type.as_deref()
        && !is_valid_event_type(et)
    {
        return Err(McpError::invalid_params("invalid event_type", None));
    }
    if let Some(imp) = item.importance {
        require_finite("importance", imp)?;
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
    runtime: Arc<LocalMemoryRuntime>,
    tool_router: ToolRouter<Self>,
    tool_mode: McpToolMode,
}

impl McpMemoryServer {
    pub fn from_runtime(runtime: Arc<LocalMemoryRuntime>) -> Self {
        Self {
            runtime,
            tool_router: Self::tool_router(),
            tool_mode: McpToolMode::Full,
        }
    }

    #[cfg(test)]
    fn new(storage: SqliteStorage) -> Self {
        Self::from_runtime(Arc::new(LocalMemoryRuntime::from_storage(storage)))
    }

    pub fn with_tool_mode(mut self, mode: McpToolMode) -> Self {
        self.tool_mode = mode;
        self
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[tool_router]
impl McpMemoryServer {
    #[tool(name = "memory_store", description = "Store memory content in SQLite and return the memory id")]
    async fn memory_store(&self, params: Parameters<StoreRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_store(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_store_batch", description = "Batch store multiple memories with optimized embedding computation. Pre-warms embedding cache with a single batched inference call for better throughput.")]
    async fn memory_store_batch(&self, params: Parameters<StoreBatchRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_store_batch(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_retrieve", description = "Retrieve stored memory content by memory id")]
    async fn memory_retrieve(&self, params: Parameters<RetrieveRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_retrieve(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_search", description = "Search stored memories. Modes: text, semantic, phrase, tag, similar. advanced=true enables the multi-phase path where supported.")]
    async fn memory_search(&self, params: Parameters<SearchRequest>) -> Result<CallToolResult, McpError> {
        tools::search::memory_search(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_list", description = "List stored memories with optional filters")]
    async fn memory_list(&self, params: Parameters<ListRequest>) -> Result<CallToolResult, McpError> {
        tools::search::memory_list(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_delete", description = "Delete a memory by id")]
    async fn memory_delete(&self, params: Parameters<DeleteRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_delete(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_update", description = "Update a memory's content, tags, importance, or metadata")]
    async fn memory_update(&self, params: Parameters<UpdateRequest>) -> Result<CallToolResult, McpError> {
        tools::manage::memory_update(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_relations", description = "Manage memory relationships and graph traversal")]
    async fn memory_relations(&self, params: Parameters<RelationsRequest>) -> Result<CallToolResult, McpError> {
        tools::manage::memory_relations(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_feedback", description = "Record feedback for a memory")]
    async fn memory_feedback(&self, params: Parameters<FeedbackRequest>) -> Result<CallToolResult, McpError> {
        tools::manage::memory_feedback(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_lifecycle", description = "Run memory maintenance and lifecycle actions")]
    async fn memory_lifecycle(&self, params: Parameters<LifecycleRequest>) -> Result<CallToolResult, McpError> {
        tools::manage::memory_lifecycle(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_checkpoint", description = "Save or resume a task checkpoint")]
    async fn memory_checkpoint(&self, params: Parameters<CheckpointRequest>) -> Result<CallToolResult, McpError> {
        tools::session::memory_checkpoint(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_remind", description = "Set, list, or dismiss reminders")]
    async fn memory_remind(&self, params: Parameters<RemindRequest>) -> Result<CallToolResult, McpError> {
        tools::session::memory_remind(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_lessons", description = "Query lesson-learned memories")]
    async fn memory_lessons(&self, params: Parameters<LessonsRequest>) -> Result<CallToolResult, McpError> {
        tools::session::memory_lessons(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_profile", description = "Read or update the cross-session user profile")]
    async fn memory_profile(&self, params: Parameters<ProfileRequest>) -> Result<CallToolResult, McpError> {
        tools::session::memory_profile(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_session_info", description = "Return the startup briefing or generated MCP protocol inventory")]
    async fn memory_session_info(&self, params: Parameters<SessionInfoRequest>) -> Result<CallToolResult, McpError> {
        tools::session::memory_session_info(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory", description = "Unified memory facade. Routes store/store_batch/retrieve/delete by action.")]
    async fn memory(&self, params: Parameters<MemoryRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_facade(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_manage", description = "Unified manage facade. Routes update/feedback/relations/lifecycle by action.")]
    async fn memory_manage(&self, params: Parameters<MemoryManageRequest>) -> Result<CallToolResult, McpError> {
        tools::facades::memory_manage(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_session", description = "Unified session facade. Routes info/checkpoint/remind/lessons/profile by action.")]
    async fn memory_session(&self, params: Parameters<MemorySessionRequest>) -> Result<CallToolResult, McpError> {
        tools::facades::memory_session(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_admin", description = "Unified admin facade. Routes health/list/export/import by action.")]
    async fn memory_admin(&self, params: Parameters<MemoryAdminFacadeRequest>) -> Result<CallToolResult, McpError> {
        tools::facades::memory_admin(self.runtime.as_ref(), &params.0).await
    }
}

impl ServerHandler for McpMemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(mcp_instructions())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tcc = ToolCallContext::new(self, request, context);
        self.tool_router.call(tcc).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let all = self.tool_router.list_all();
        let tools = match self.tool_mode {
            McpToolMode::Full => all,
            McpToolMode::Minimal => {
                let minimal = facade_tool_names();
                all.into_iter()
                    .filter(|tool| minimal.contains(&tool.name.as_ref()))
                    .collect()
            }
        };
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let tool = self.tool_router.get(name)?;
        if self.tool_mode == McpToolMode::Minimal
            && !facade_tool_names().contains(&tool.name.as_ref())
        {
            return None;
        }
        Some(tool.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_contract_matches_router_names() {
        let router = McpMemoryServer::tool_router();
        let mut router_names: Vec<&str> = router.list_all().iter().map(|tool| tool.name.as_ref()).collect();
        let mut contract_names: Vec<&str> = TOOL_REGISTRY.iter().map(|tool| tool.name).collect();
        router_names.sort_unstable();
        contract_names.sort_unstable();
        assert_eq!(contract_names, router_names, "owned MCP contract and tool router drifted");
    }

    #[test]
    fn contract_names_and_categories_are_valid() {
        let mut seen = std::collections::HashSet::new();
        for tool in TOOL_REGISTRY {
            assert!(seen.insert(tool.name), "duplicate MCP contract tool: {}", tool.name);
            assert!(CATEGORY_ORDER.contains(&tool.category), "unknown category '{}' for {}", tool.category, tool.name);
        }
    }

    #[test]
    fn facade_surface_is_the_minimal_mode_contract() {
        assert_eq!(facade_tool_names(), vec!["memory", "memory_manage", "memory_session", "memory_admin"]);
        assert_eq!(legacy_tool_count(), 15);
        assert_eq!(TOOL_REGISTRY.len(), 19);
    }

    #[test]
    fn generated_protocol_contains_every_contract_tool_and_counts() {
        let markdown = generate_protocol_markdown();
        for tool in TOOL_REGISTRY {
            assert!(markdown.contains(tool.name), "protocol missing {}", tool.name);
        }
        assert!(markdown.contains("Full: 19 total tools (4 facade + 15 legacy compatibility)"));
        assert!(markdown.contains("Minimal: 4 facade tools"));
    }

    #[test]
    fn initialization_instructions_derive_interface_counts() {
        let instructions = mcp_instructions();
        assert!(instructions.contains("4 facade tools"));
        assert!(instructions.contains("15 legacy compatibility tools"));
        assert!(!instructions.contains("16 legacy"));
        assert!(instructions.contains("MCP is a thin optional transport"));
    }

    #[test]
    fn tool_registry_json_matches_contract() {
        let value = tool_registry_json();
        assert_eq!(value["tool_count"], 19);
        assert_eq!(value["facade_tool_count"], 4);
        assert_eq!(value["legacy_tool_count"], 15);
        let tools = value["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), TOOL_REGISTRY.len());
    }
}