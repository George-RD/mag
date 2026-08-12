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

// ── Submodule declarations ──

mod request_types;
mod tools;
pub(crate) mod validation;

use request_types::*;
use validation::require_finite;

// ──────────────────────── Tool mode ────────────────────────

/// Controls which tools are advertised to MCP clients.
///
/// - `Full` (default): all tools in [`TOOL_REGISTRY`].
/// - `Minimal`: only unified facade tools from [`TOOL_REGISTRY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum McpToolMode {
    #[default]
    Full,
    Minimal,
}

// ──────────────────────── Tool Registry ────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolSurface {
    Legacy,
    Facade,
}

/// Metadata for a single MCP tool, used to generate protocol docs and CLI output.
pub struct ToolMeta {
    pub name: &'static str,
    pub summary: &'static str,
    pub category: &'static str,
    surface: ToolSurface,
}

/// Canonical registry of all MCP tools. Tool advertisement modes, counts,
/// initialization instructions, protocol docs, and CLI protocol output derive
/// from this registry. A parity test keeps it aligned with `#[tool(...)]` attrs.
pub const TOOL_REGISTRY: &[ToolMeta] = &[
    // Storage & Retrieval
    ToolMeta {
        name: "memory",
        summary: "Unified facade (Wave 2 preview): store/store_batch/retrieve/delete via action field",
        category: "Storage & Retrieval",
        surface: ToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_store",
        summary: "Store new memory content with tags, importance, metadata",
        category: "Storage & Retrieval",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_store_batch",
        summary: "Batch store multiple memories with optimized embedding",
        category: "Storage & Retrieval",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_retrieve",
        summary: "Retrieve a memory by ID",
        category: "Storage & Retrieval",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_delete",
        summary: "Delete a memory by ID",
        category: "Storage & Retrieval",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_update",
        summary: "Update content, tags, importance, or metadata",
        category: "Storage & Retrieval",
        surface: ToolSurface::Legacy,
    },
    // Search & Listing
    ToolMeta {
        name: "memory_search",
        summary: "Unified search (mode: text|semantic|phrase|tag|similar, advanced: bool)",
        category: "Search & Listing",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_list",
        summary: "List memories (sort: created|recent)",
        category: "Search & Listing",
        surface: ToolSurface::Legacy,
    },
    // Relationships & Graph
    ToolMeta {
        name: "memory_relations",
        summary: "Manage relationships (action: list|add|traverse|version_chain)",
        category: "Relationships & Graph",
        surface: ToolSurface::Legacy,
    },
    // Lifecycle & Feedback
    ToolMeta {
        name: "memory_feedback",
        summary: "Record feedback (helpful/unhelpful/outdated)",
        category: "Lifecycle & Feedback",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_lifecycle",
        summary: "System maintenance (action: sweep|health|consolidate|compact|auto_compact|fts_rebuild|clear_session)",
        category: "Lifecycle & Feedback",
        surface: ToolSurface::Legacy,
    },
    // Cross-Session
    ToolMeta {
        name: "memory_checkpoint",
        summary: "Task checkpoints (action: save|resume)",
        category: "Cross-Session",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_remind",
        summary: "Set, list, or dismiss reminders",
        category: "Cross-Session",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_lessons",
        summary: "Query lesson_learned memories",
        category: "Cross-Session",
        surface: ToolSurface::Legacy,
    },
    ToolMeta {
        name: "memory_profile",
        summary: "Read/update user profile",
        category: "Cross-Session",
        surface: ToolSurface::Legacy,
    },
    // System
    ToolMeta {
        name: "memory_session_info",
        summary: "Welcome briefing or protocol (mode: welcome|protocol)",
        category: "System",
        surface: ToolSurface::Legacy,
    },
    // Unified facades
    ToolMeta {
        name: "memory_manage",
        summary: "Unified manage facade: update/feedback/relations/lifecycle via action field",
        category: "Storage & Retrieval",
        surface: ToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_session",
        summary: "Unified session facade: info/checkpoint/remind/lessons/profile via action field",
        category: "Cross-Session",
        surface: ToolSurface::Facade,
    },
    ToolMeta {
        name: "memory_admin",
        summary: "Unified admin facade: health/list/export/import via action field (Wave 2 — replaces legacy admin+list)",
        category: "System",
        surface: ToolSurface::Facade,
    },
];

/// Category display order for protocol markdown output.
const CATEGORY_ORDER: &[&str] = &[
    "Storage & Retrieval",
    "Search & Listing",
    "Relationships & Graph",
    "Lifecycle & Feedback",
    "Cross-Session",
    "System",
];

fn tool_names(surface: ToolSurface) -> Vec<&'static str> {
    TOOL_REGISTRY
        .iter()
        .filter(|tool| tool.surface == surface)
        .map(|tool| tool.name)
        .collect()
}

fn facade_tool_names() -> Vec<&'static str> {
    tool_names(ToolSurface::Facade)
}

fn legacy_tool_count() -> usize {
    TOOL_REGISTRY
        .iter()
        .filter(|tool| tool.surface == ToolSurface::Legacy)
        .count()
}

// ──────────────────────── MCP Instructions ────────────────────────

/// Instructions returned in the MCP `initialize` handshake.
///
/// MCP is an optional transport for hosts that need it. MAG's CLI remains the
/// canonical external interface; both surfaces share the same runtime workflows.
fn mcp_instructions() -> String {
    let facades = facade_tool_names();
    format!(
        "MAG — persistent memory for AI agents. MCP is an optional transport over the same runtime workflows as MAG's canonical CLI.\n\n\
## 99999 — SESSION START (do this FIRST, every session)\n\
Call memory_session(action=\"info\", info_mode=\"welcome\") BEFORE your first substantive response when MCP is your MAG integration.\n\n\
## 99999 — STORE DECISIONS AND LESSONS\n\
After completing non-trivial work, persist architectural decisions, bug root causes and fixes, user preferences and corrections, and reusable workflow lessons.\n\n\
## 9999 — SEARCH BEFORE DECIDING\n\
Before recurring debugging or architectural decisions, check prior memory.\n\n\
## 9999 — DO NOT ANNOUNCE MEMORY OPERATIONS\n\
Use memory operations silently and weave recalled context naturally into responses.\n\n\
## 999 — TOOL REFERENCE\n\
Call memory_session(action=\"info\", info_mode=\"protocol\") for the generated tool inventory.\n\n\
## 999 — DIAGNOSTICS\n\
If tool calls fail, ask the user to run: mag doctor\n\n\
## 999 — UNIFIED MCP INTERFACE\n\
Prefer the {} facade tools for new MCP integrations: {}. Full mode additionally exposes {} legacy compatibility tools. Use `--mcp-tools=minimal` to advertise only the facades.\n",
        facades.len(),
        facades.join(", "),
        legacy_tool_count(),
    )
}

/// Generate the protocol markdown from [`TOOL_REGISTRY`].
pub fn generate_protocol_markdown() -> String {
    let count = TOOL_REGISTRY.len();
    let mut out = format!("# MAG Protocol\n\n## Available Tools ({count})\n");

    for &cat in CATEGORY_ORDER {
        let _ = write!(out, "\n### {cat}\n");
        for tool in TOOL_REGISTRY.iter().filter(|t| t.category == cat) {
            let _ = writeln!(out, "- **{}** — {}", tool.name, tool.summary);
        }
    }

    let facades = facade_tool_names();
    let _ = write!(
        out,
        "\n## Interface Modes\n- Minimal: {} facade tools ({})\n- Full: {} total tools ({} facade + {} legacy compatibility)\n",
        facades.len(),
        facades.join(", "),
        TOOL_REGISTRY.len(),
        facades.len(),
        legacy_tool_count(),
    );

    out.push_str(
        "\n## Usage Guidelines\n\
         - Prefer the MAG CLI when the host can execute it; MCP is an optional transport\n\
         - Call **memory_session** with `action=\"info\"` and `info_mode=\"welcome\"` at session start in MCP-only hosts\n\
         - Prefer the unified facade tools for new MCP integrations\n\
         - Keep legacy tools for compatibility only\n",
    );

    out
}

/// Return protocol metadata derived from [`TOOL_REGISTRY`]. Used by the CLI
/// `protocol` sub-command.
pub fn tool_registry_json() -> serde_json::Value {
    let names: Vec<&str> = TOOL_REGISTRY.iter().map(|t| t.name).collect();
    let facades = facade_tool_names();
    json!({
        "tools": names,
        "tool_count": TOOL_REGISTRY.len(),
        "facade_tools": facades,
        "facade_tool_count": tool_names(ToolSurface::Facade).len(),
        "legacy_tool_count": legacy_tool_count(),
    })
}

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

// ──────────────────────── Server struct ────────────────────────

#[derive(Clone)]
pub struct McpMemoryServer {
    runtime: Arc<LocalMemoryRuntime>,
    tool_router: ToolRouter<Self>,
    tool_mode: McpToolMode,
}

impl McpMemoryServer {
    /// Creates the optional MCP transport around the entrypoint-owned runtime.
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

// ──────────────────────── Tool router (thin delegation wrappers) ────────────────────────

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
        tools::storage::memory_store(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_store_batch",
        description = "Batch store multiple memories with optimized embedding computation. Pre-warms embedding cache with a single batched inference call for better throughput."
    )]
    async fn memory_store_batch(
        &self,
        params: Parameters<StoreBatchRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::storage::memory_store_batch(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_retrieve",
        description = "Retrieve stored memory content by memory id"
    )]
    async fn memory_retrieve(
        &self,
        params: Parameters<RetrieveRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::storage::memory_retrieve(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_search",
        description = "Search stored memories. Modes: 'text' (default, FTS5), 'semantic' (embedding similarity), 'phrase' (exact substring), 'tag' (AND-match tags), 'similar' (find similar to memory_id). advanced=true enables multi-phase retrieval; only 'text' and 'semantic' modes support it ('phrase', 'tag', 'similar' always use their standard paths). Text mode defaults to advanced=true. Required params vary by mode: text/semantic/phrase need 'query', tag needs 'tags', similar needs 'memory_id'."
    )]
    async fn memory_search(
        &self,
        params: Parameters<SearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::memory_search(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_list",
        description = "List stored memories. Sort: 'created' (default, paginated by creation time with offset) or 'recent' (recently accessed)."
    )]
    async fn memory_list(
        &self,
        params: Parameters<ListRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::search::memory_list(self.runtime.as_ref(), &params.0).await
    }

    #[tool(name = "memory_delete", description = "Delete a memory by its id")]
    async fn memory_delete(
        &self,
        params: Parameters<DeleteRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::storage::memory_delete(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_update",
        description = "Update content and optionally tags of an existing memory"
    )]
    async fn memory_update(
        &self,
        params: Parameters<UpdateRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::lifecycle::memory_update(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_relations",
        description = "Manage memory relationships and graph traversal. Actions: 'list' (default, get relationships for a memory), 'add' (create directed relationship), 'traverse' (BFS graph traversal), 'version_chain' (full version history)."
    )]
    async fn memory_relations(
        &self,
        params: Parameters<RelationsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::relations::memory_relations(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_feedback",
        description = "Record user feedback signal for a memory"
    )]
    async fn memory_feedback(
        &self,
        params: Parameters<FeedbackRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::lifecycle::memory_feedback(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_lifecycle",
        description = "System maintenance. Actions: 'sweep' (default, expire TTL-based memories), 'health' (diagnostic with thresholds, incl. FTS5 sync status), 'consolidate' (prune stale data), 'compact' (merge near-duplicates), 'auto_compact' (embedding-based dedup), 'fts_rebuild' (rebuild the full-text index from source), 'clear_session' (remove session data), 'backup' (create binary backup), 'backup_list' (list available backups)."
    )]
    async fn memory_lifecycle(
        &self,
        params: Parameters<LifecycleRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::lifecycle::memory_lifecycle(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_checkpoint",
        description = "Manage cross-session task checkpoints. Actions: 'save' (default, save a checkpoint) or 'resume' (retrieve prior checkpoints)."
    )]
    async fn memory_checkpoint(
        &self,
        params: Parameters<CheckpointRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::session::memory_checkpoint(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_remind",
        description = "Set, list, or dismiss reminders"
    )]
    async fn memory_remind(
        &self,
        params: Parameters<RemindRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::session::memory_remind(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_lessons",
        description = "Query lesson_learned memories for a task or project"
    )]
    async fn memory_lessons(
        &self,
        params: Parameters<LessonsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::session::memory_lessons(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_profile",
        description = "Read or update the cross-session user profile"
    )]
    async fn memory_profile(
        &self,
        params: Parameters<ProfileRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::session::memory_profile(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_session_info",
        description = "Session-oriented information. mode='welcome' (default) returns the startup briefing; mode='protocol' returns the tool inventory and usage guidelines."
    )]
    async fn memory_session_info(
        &self,
        params: Parameters<SessionInfoRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::session::memory_session_info(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory",
        description = "Unified memory facade (Wave 2 preview). Routes to store/store_batch/retrieve/delete based on `action` field (default: \"store\"). Use this single tool instead of the four individual tools when you prefer a collapsed interface."
    )]
    async fn memory(&self, params: Parameters<MemoryRequest>) -> Result<CallToolResult, McpError> {
        tools::storage::memory_facade(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_manage",
        description = "Unified manage facade (Wave 2). Routes to update/feedback/relations/lifecycle based on `action` field (default: \"update\"). Sub-actions: relations_action (list|add|traverse|version_chain), lifecycle_action (sweep|health|consolidate|compact|auto_compact|fts_rebuild|clear_session|backup|backup_list)."
    )]
    async fn memory_manage(
        &self,
        params: Parameters<MemoryManageRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::facades::memory_manage(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_session",
        description = "Unified session facade (Wave 2). Routes to info/checkpoint/remind/lessons/profile based on `action` field (default: \"info\"). Sub-actions: checkpoint_action (save|resume), remind_action (set|list|dismiss), profile_action (read|update)."
    )]
    async fn memory_session(
        &self,
        params: Parameters<MemorySessionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::facades::memory_session(self.runtime.as_ref(), &params.0).await
    }

    #[tool(
        name = "memory_admin",
        description = "Unified admin facade (Wave 2). Routes to health/list/export/import based on `action` field (default: \"health\"). Use list_limit and list_event_type for the list action to avoid ambiguity with health parameters."
    )]
    async fn memory_admin(
        &self,
        params: Parameters<MemoryAdminFacadeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::facades::memory_admin(self.runtime.as_ref(), &params.0).await
    }
}

// ──────────────────────── ServerHandler impl ────────────────────────

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

    /// Returns the tool list filtered by the configured [`McpToolMode`].
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let all = self.tool_router.list_all();
        let tools = match self.tool_mode {
            McpToolMode::Full => all,
            McpToolMode::Minimal => {
                let facade_names = facade_tool_names();
                all.into_iter()
                    .filter(|tool| facade_names.contains(&tool.name.as_ref()))
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

// ──────────────────────── Tests ────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_registry_matches_router() {
        let router = McpMemoryServer::tool_router();
        let registered_tools = router.list_all();
        let mut registered: Vec<&str> = registered_tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect();
        let mut documented: Vec<&str> = TOOL_REGISTRY.iter().map(|tool| tool.name).collect();
        registered.sort_unstable();
        documented.sort_unstable();
        assert_eq!(
            documented, registered,
            "TOOL_REGISTRY and #[tool] router names must stay in parity"
        );
    }

    #[test]
    fn tool_registry_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for tool in TOOL_REGISTRY {
            assert!(
                seen.insert(tool.name),
                "duplicate tool name in TOOL_REGISTRY: {}",
                tool.name
            );
        }
    }

    #[test]
    fn tool_registry_categories_in_order() {
        for tool in TOOL_REGISTRY {
            assert!(
                CATEGORY_ORDER.contains(&tool.category),
                "tool '{}' has category '{}' not in CATEGORY_ORDER",
                tool.name,
                tool.category
            );
        }
    }

    #[test]
    fn facade_and_legacy_counts_are_derived_from_registry() {
        assert_eq!(
            facade_tool_names(),
            vec!["memory", "memory_manage", "memory_session", "memory_admin"]
        );
        assert_eq!(legacy_tool_count(), 15);
        assert_eq!(TOOL_REGISTRY.len(), 19);
    }

    #[test]
    fn generate_protocol_markdown_contains_all_tools_and_modes() {
        let md = generate_protocol_markdown();
        for tool in TOOL_REGISTRY {
            assert!(
                md.contains(tool.name),
                "protocol markdown missing tool: {}",
                tool.name
            );
        }
        assert!(md.contains("## Available Tools (19)"));
        assert!(md.contains("Minimal: 4 facade tools"));
        assert!(md.contains("Full: 19 total tools (4 facade + 15 legacy compatibility)"));
    }

    #[test]
    fn initialization_instructions_use_derived_contract_counts() {
        let instructions = mcp_instructions();
        assert!(instructions.contains("4 facade tools"));
        assert!(instructions.contains("15 legacy compatibility tools"));
        assert!(!instructions.contains("16 legacy"));
        assert!(instructions.contains("canonical CLI"));
        assert!(instructions.contains("optional transport"));
    }

    #[test]
    fn tool_registry_json_matches_registry() {
        let val = tool_registry_json();
        let tools = val["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools.len(), TOOL_REGISTRY.len());
        assert_eq!(val["tool_count"], TOOL_REGISTRY.len());
        assert_eq!(val["facade_tool_count"], 4);
        assert_eq!(val["legacy_tool_count"], 15);
        for (i, tool) in TOOL_REGISTRY.iter().enumerate() {
            assert_eq!(
                tools[i].as_str().expect("tool name should be a string"),
                tool.name
            );
        }
    }

    #[test]
    fn minimal_mode_is_exactly_the_facade_surface() {
        let router = McpMemoryServer::tool_router();
        let all = router.list_all();
        let facade_names = facade_tool_names();
        let minimal_names: Vec<&str> = all
            .iter()
            .filter(|tool| facade_names.contains(&tool.name.as_ref()))
            .map(|tool| tool.name.as_ref())
            .collect();
        assert_eq!(minimal_names.len(), facade_names.len());
        for name in &facade_names {
            assert!(minimal_names.contains(name));
        }
    }
}
