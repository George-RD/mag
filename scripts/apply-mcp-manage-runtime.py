from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text()
    actual = text.count(old)
    if actual != expected:
        raise RuntimeError(
            f"{path}: expected {expected} occurrences of {old!r}, found {actual}"
        )
    file_path.write_text(text.replace(old, new))


def replace_between(path: str, start: str, end: str, replacement: str) -> None:
    file_path = Path(path)
    text = file_path.read_text()
    if text.count(start) != 1 or text.count(end) != 1:
        raise RuntimeError(f"{path}: section markers are not unique")
    start_index = text.index(start)
    end_index = text.index(end)
    if start_index >= end_index:
        raise RuntimeError(f"{path}: section markers are out of order")
    file_path.write_text(text[:start_index] + replacement + text[end_index:])


# Add the only two storage capabilities still missing from the selected runtime.
replace_exact(
    "src/local_memory_runtime.rs",
    '''    /// Returns stored relationships without changing ordering or payload semantics.\n    pub async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>> {\n        self.storage.get_relationships(memory_id).await\n    }\n\n''',
    '''    /// Returns stored relationships without changing ordering or payload semantics.\n    pub async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>> {\n        self.storage.get_relationships(memory_id).await\n    }\n\n    /// Adds a directed relationship without changing ID, weight, or metadata semantics.\n    pub async fn add_relationship(\n        &self,\n        source_id: &str,\n        target_id: &str,\n        rel_type: &str,\n        weight: f64,\n        metadata: &serde_json::Value,\n    ) -> Result<String> {\n        self.storage\n            .add_relationship(source_id, target_id, rel_type, weight, metadata)\n            .await\n    }\n\n''',
)
replace_exact(
    "src/local_memory_runtime.rs",
    '''    /// Clears one session without changing relationship cleanup semantics.\n    pub async fn clear_session(&self, session_id: &str) -> Result<usize> {\n''',
    '''    /// Runs automatic compaction without changing threshold or dry-run semantics.\n    pub async fn auto_compact(\n        &self,\n        count_threshold: usize,\n        dry_run: bool,\n    ) -> Result<serde_json::Value> {\n        self.storage.auto_compact(count_threshold, dry_run).await\n    }\n\n    /// Clears one session without changing relationship cleanup semantics.\n    pub async fn clear_session(&self, session_id: &str) -> Result<usize> {\n''',
)

# Route the legacy lifecycle handlers through LocalMemoryRuntime.
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    '''use crate::memory_core::storage::SqliteStorage;\nuse crate::memory_core::{\n    BackupManager, EventType, ExpirationSweeper, FeedbackRecorder, MaintenanceManager,\n    MemoryUpdate, Updater, is_valid_event_type,\n};\n''',
    '''use crate::LocalMemoryRuntime;\nuse crate::memory_core::{EventType, MemoryUpdate, is_valid_event_type};\n''',
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "    storage: &SqliteStorage,\n",
    "    runtime: &LocalMemoryRuntime,\n",
    expected=3,
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "    <SqliteStorage as Updater>::update(storage, &req.id, &update)\n",
    "    runtime.update(&req.id, &update)\n",
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    '''    let result = <SqliteStorage as FeedbackRecorder>::record_feedback(\n        storage,\n        &req.memory_id,\n        rating,\n        req.reason.as_deref(),\n    )\n''',
    '''    let result = runtime\n        .record_feedback(&req.memory_id, rating, req.reason.as_deref())\n''',
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "            let swept_count = <SqliteStorage as ExpirationSweeper>::sweep_expired(storage)\n",
    "            let swept_count = runtime.sweep_expired()\n",
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "storage.",
    "runtime.",
    expected=2,
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "storage\n                .",
    "runtime\n                .",
    expected=4,
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "            let info = <SqliteStorage as BackupManager>::create_backup(storage)\n",
    "            let info = runtime.create_backup()\n",
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "            let _ = <SqliteStorage as BackupManager>::rotate_backups(storage, 5).await;\n",
    "            let _ = runtime.rotate_backups(5).await;\n",
)
replace_exact(
    "src/mcp/tools/lifecycle.rs",
    "            let backups = <SqliteStorage as BackupManager>::list_backups(storage)\n",
    "            let backups = runtime.list_backups()\n",
)

# Route relationship and graph behavior through the same runtime.
replace_exact(
    "src/mcp/tools/relations.rs",
    '''use crate::memory_core::storage::SqliteStorage;\nuse crate::memory_core::{GraphTraverser, RelationshipQuerier, Retriever, VersionChainQuerier};\n''',
    "use crate::LocalMemoryRuntime;\n",
)
replace_exact(
    "src/mcp/tools/relations.rs",
    "    storage: &SqliteStorage,\n",
    "    runtime: &LocalMemoryRuntime,\n",
)
replace_exact(
    "src/mcp/tools/relations.rs",
    "            let rels = storage.get_relationships(id).await.map_err(|e| {\n",
    "            let rels = runtime.get_relationships(id).await.map_err(|e| {\n",
)
replace_exact(
    "src/mcp/tools/relations.rs",
    "            let rel_id = storage\n                .add_relationship(source_id, target_id, rel_type, weight, &metadata)\n",
    "            let rel_id = runtime\n                .add_relationship(source_id, target_id, rel_type, weight, &metadata)\n",
)
replace_exact(
    "src/mcp/tools/relations.rs",
    "            storage.retrieve(id).await.map_err(|e| {\n",
    "            runtime.retrieve(id).await.map_err(|e| {\n",
)
replace_exact(
    "src/mcp/tools/relations.rs",
    '''            let nodes = <SqliteStorage as GraphTraverser>::traverse(\n                storage, id, max_hops, min_weight, None,\n            )\n''',
    '''            let nodes = runtime\n                .traverse(id, max_hops, min_weight, None)\n''',
)
replace_exact(
    "src/mcp/tools/relations.rs",
    "            let results = storage.get_version_chain(id).await.map_err(|e| {\n",
    "            let results = runtime.version_chain(id).await.map_err(|e| {\n",
)

# Collapse the unified manage facade onto the legacy action handlers. Only the
# facade-specific presence and unknown-action wording remains at this boundary.
replace_exact(
    "src/mcp/tools/facades.rs",
    '''use crate::LocalMemoryRuntime;\nuse crate::memory_core::storage::SqliteStorage;\nuse crate::memory_core::{\n    BackupManager, CheckpointInput, EventType, ExpirationSweeper, FeedbackRecorder, GraphTraverser,\n    MaintenanceManager, MemoryUpdate, RelationshipQuerier, Retriever, SearchOptions, Updater,\n    VersionChainQuerier, WelcomeOptions, is_valid_event_type,\n};\n\nuse super::super::request_types::{\n    MemoryAdminFacadeRequest, MemoryManageRequest, MemorySessionRequest,\n};\nuse super::super::validation::{MAX_RESULT_LIMIT, require_finite};\nuse super::super::{generate_protocol_markdown, serialize_results};\n''',
    '''use crate::LocalMemoryRuntime;\nuse crate::memory_core::{\n    CheckpointInput, EventType, SearchOptions, WelcomeOptions, is_valid_event_type,\n};\n\nuse super::super::request_types::{\n    FeedbackRequest, LifecycleRequest, MemoryAdminFacadeRequest, MemoryManageRequest,\n    MemorySessionRequest, RelationsRequest, UpdateRequest,\n};\nuse super::super::validation::{MAX_RESULT_LIMIT, require_finite};\nuse super::super::{generate_protocol_markdown, serialize_results};\nuse super::{lifecycle, relations};\n''',
)

MANAGE_SECTION = '''// ── memory_manage (unified facade) ──\n\npub(crate) async fn memory_manage(\n    runtime: &LocalMemoryRuntime,\n    req: &MemoryManageRequest,\n) -> Result<CallToolResult, McpError> {\n    let action = req.action.as_deref().unwrap_or("update");\n\n    match action {\n        "update" => {\n            let id = req.id.as_deref().ok_or_else(|| {\n                McpError::invalid_params("id is required for action=update", None)\n            })?;\n            let request = UpdateRequest {\n                id: id.to_string(),\n                content: req.content.clone(),\n                tags: req.tags.clone(),\n                importance: req.importance,\n                metadata: req.metadata.clone(),\n                event_type: req.event_type.clone(),\n                priority: req.priority,\n            };\n            lifecycle::memory_update(runtime, &request).await\n        }\n        "feedback" => {\n            let memory_id = req.memory_id.as_deref().ok_or_else(|| {\n                McpError::invalid_params("memory_id is required for action=feedback", None)\n            })?;\n            let rating = req.rating.as_deref().ok_or_else(|| {\n                McpError::invalid_params("rating is required for action=feedback", None)\n            })?;\n            let request = FeedbackRequest {\n                memory_id: memory_id.to_string(),\n                rating: rating.to_string(),\n                reason: req.reason.clone(),\n            };\n            lifecycle::memory_feedback(runtime, &request).await\n        }\n        "relations" => {\n            let sub = req.relations_action.as_deref().unwrap_or("list");\n            match sub {\n                "list" => {\n                    req.id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "id is required for relations_action=list",\n                            None,\n                        )\n                    })?;\n                }\n                "add" => {\n                    req.source_id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "source_id is required for relations_action=add",\n                            None,\n                        )\n                    })?;\n                    req.target_id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "target_id is required for relations_action=add",\n                            None,\n                        )\n                    })?;\n                    req.rel_type.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "rel_type is required for relations_action=add",\n                            None,\n                        )\n                    })?;\n                }\n                "traverse" => {\n                    req.id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "id is required for relations_action=traverse",\n                            None,\n                        )\n                    })?;\n                }\n                "version_chain" => {\n                    req.id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "id is required for relations_action=version_chain",\n                            None,\n                        )\n                    })?;\n                }\n                other => {\n                    return Err(McpError::invalid_params(\n                        format!(\n                            "unknown relations_action: {other} (expected list|add|traverse|version_chain)"\n                        ),\n                        None,\n                    ));\n                }\n            }\n\n            let request = RelationsRequest {\n                action: Some(sub.to_string()),\n                id: req.id.clone(),\n                source_id: req.source_id.clone(),\n                target_id: req.target_id.clone(),\n                rel_type: req.rel_type.clone(),\n                weight: req.weight,\n                metadata: req.metadata.clone(),\n                max_hops: req.max_hops,\n                min_weight: req.min_weight,\n            };\n            relations::memory_relations(runtime, &request).await\n        }\n        "lifecycle" => {\n            let sub = req.lifecycle_action.as_deref().unwrap_or("sweep");\n            match sub {\n                "clear_session" => {\n                    req.session_id.as_deref().ok_or_else(|| {\n                        McpError::invalid_params(\n                            "session_id is required for lifecycle_action=clear_session",\n                            None,\n                        )\n                    })?;\n                }\n                "sweep" | "health" | "consolidate" | "compact" | "auto_compact"\n                | "fts_rebuild" | "backup" | "backup_list" => {}\n                other => {\n                    return Err(McpError::invalid_params(\n                        format!(\n                            "unknown lifecycle_action: {other} (expected sweep|health|consolidate|compact|auto_compact|fts_rebuild|clear_session|backup|backup_list)"\n                        ),\n                        None,\n                    ));\n                }\n            }\n\n            let request = LifecycleRequest {\n                action: Some(sub.to_string()),\n                warn_mb: req.warn_mb,\n                critical_mb: req.critical_mb,\n                max_nodes: req.max_nodes,\n                prune_days: req.prune_days,\n                max_summaries: req.max_summaries,\n                event_type: req.event_type.clone(),\n                similarity_threshold: req.similarity_threshold,\n                min_cluster_size: req.min_cluster_size,\n                dry_run: req.dry_run,\n                session_id: req.session_id.clone(),\n                count_threshold: req.count_threshold,\n            };\n            lifecycle::memory_lifecycle(runtime, &request).await\n        }\n        other => Err(McpError::invalid_params(\n            format!("unknown action: {other} (expected update|feedback|relations|lifecycle)"),\n            None,\n        )),\n    }\n}\n\n'''
replace_between(
    "src/mcp/tools/facades.rs",
    "// ── memory_manage (unified facade) ──\n",
    "// ── memory_session (unified facade) ──\n",
    MANAGE_SECTION,
)

# The server now owns one production composition root instead of a runtime plus
# a second direct storage handle.
replace_exact(
    "src/mcp/mod.rs",
    "    storage: SqliteStorage,\n",
    "",
)
replace_exact(
    "src/mcp/mod.rs",
    "        Self {\n            storage,\n            runtime,\n",
    "        Self {\n            runtime,\n",
)
replace_exact(
    "src/mcp/mod.rs",
    "&self.storage",
    "self.runtime.as_ref()",
    expected=5,
)

# Fail closed if a direct storage boundary remains in production MCP tools.
for path in [
    "src/mcp/tools/lifecycle.rs",
    "src/mcp/tools/relations.rs",
    "src/mcp/tools/facades.rs",
]:
    text = Path(path).read_text()
    if "SqliteStorage" in text or "&self.storage" in text:
        raise RuntimeError(f"{path}: direct storage boundary remains")

server_text = Path("src/mcp/mod.rs").read_text()
if "&self.storage" in server_text or "    storage: SqliteStorage," in server_text:
    raise RuntimeError("src/mcp/mod.rs: duplicate server storage remains")
