use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    BackupManager, CheckpointInput, CheckpointManager, EventType, ExpirationSweeper,
    FeedbackRecorder, GraphTraverser, LessonQuerier, Lister, MaintenanceManager, MemoryUpdate,
    ProfileManager, Recents, RelationshipQuerier, ReminderManager, Retriever, SearchOptions,
    StatsProvider, Updater, VersionChainQuerier, WelcomeProvider, is_valid_event_type,
};

use super::super::request_types::{
    MemoryAdminFacadeRequest, MemoryManageRequest, MemorySessionRequest,
};
use super::super::validation::{MAX_RESULT_LIMIT, require_finite};
use super::super::{generate_protocol_markdown, serialize_results};

// ── memory_manage (unified facade) ──

pub(crate) async fn memory_manage(
    storage: &SqliteStorage,
    req: &MemoryManageRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("update");

    match action {
        "update" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=update", None)
            })?;
            if req.content.is_none()
                && req.tags.is_none()
                && req.importance.is_none()
                && req.metadata.is_none()
                && req.event_type.is_none()
                && req.priority.is_none()
            {
                return Err(McpError::invalid_params(
                    "at least one of content, tags, importance, metadata, event_type, or priority must be provided",
                    None,
                ));
            }
            if let Some(event_type) = req.event_type.as_deref()
                && !is_valid_event_type(event_type)
            {
                return Err(McpError::invalid_params("invalid event_type", None));
            }
            let update = MemoryUpdate {
                content: req.content.clone(),
                tags: req.tags.clone(),
                importance: req.importance,
                metadata: req.metadata.clone(),
                event_type: EventType::from_optional(req.event_type.as_deref()),
                priority: req.priority,
            };
            <SqliteStorage as Updater>::update(storage, id, &update)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to update memory: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "id": id, "updated": true }).to_string(),
            )]))
        }
        "feedback" => {
            let memory_id = req.memory_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("memory_id is required for action=feedback", None)
            })?;
            let rating = req.rating.as_deref().ok_or_else(|| {
                McpError::invalid_params("rating is required for action=feedback", None)
            })?;
            if !matches!(rating, "helpful" | "unhelpful" | "outdated") {
                return Err(McpError::invalid_params("invalid rating", None));
            }
            let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
                storage,
                memory_id,
                rating,
                req.reason.as_deref(),
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to record feedback: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({"memory_id": memory_id, "feedback": result}).to_string(),
            )]))
        }
        "relations" => {
            let sub = req.relations_action.as_deref().unwrap_or("list");
            match sub {
                "list" => {
                    let id = req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params("id is required for relations_action=list", None)
                    })?;
                    let rels = storage.get_relationships(id).await.map_err(|e| {
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
                    let source_id = req.source_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "source_id is required for relations_action=add",
                            None,
                        )
                    })?;
                    let target_id = req.target_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "target_id is required for relations_action=add",
                            None,
                        )
                    })?;
                    let rel_type = req.rel_type.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "rel_type is required for relations_action=add",
                            None,
                        )
                    })?;
                    let weight = req.weight.unwrap_or(1.0);
                    require_finite("weight", weight)?;
                    if !(0.0..=1.0).contains(&weight) {
                        return Err(McpError::invalid_params(
                            "weight must be between 0.0 and 1.0",
                            None,
                        ));
                    }
                    let metadata = req
                        .metadata
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let rel_id = storage
                        .add_relationship(source_id, target_id, rel_type, weight, &metadata)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("failed to add relationship: {e}"),
                                None,
                            )
                        })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "id": rel_id, "source_id": source_id, "target_id": target_id, "rel_type": rel_type, "weight": weight, "metadata": metadata }).to_string(),
                    )]))
                }
                "traverse" => {
                    let id = req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "id is required for relations_action=traverse",
                            None,
                        )
                    })?;
                    storage.retrieve(id).await.map_err(|e| {
                        McpError::internal_error(
                            format!("memory not found for traversal: {e}"),
                            None,
                        )
                    })?;
                    let max_hops = req.max_hops.unwrap_or(2);
                    if !(1..=5).contains(&max_hops) {
                        return Err(McpError::invalid_params(
                            "max_hops must be between 1 and 5",
                            None,
                        ));
                    }
                    let min_weight = req.min_weight.unwrap_or(0.0);
                    require_finite("min_weight", min_weight)?;
                    if !(0.0..=1.0).contains(&min_weight) {
                        return Err(McpError::invalid_params(
                            "min_weight must be between 0.0 and 1.0",
                            None,
                        ));
                    }
                    let nodes = <SqliteStorage as GraphTraverser>::traverse(
                        storage, id, max_hops, min_weight, None,
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
                    let id = req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "id is required for relations_action=version_chain",
                            None,
                        )
                    })?;
                    let results = storage.get_version_chain(id).await.map_err(|e| {
                        McpError::internal_error(format!("failed to get version chain: {e}"), None)
                    })?;
                    let payload = serialize_results(results)?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "chain": payload }).to_string(),
                    )]))
                }
                other => Err(McpError::invalid_params(
                    format!(
                        "unknown relations_action: {other} (expected list|add|traverse|version_chain)"
                    ),
                    None,
                )),
            }
        }
        "lifecycle" => {
            let sub = req.lifecycle_action.as_deref().unwrap_or("sweep");
            match sub {
                "sweep" => {
                    let swept_count = <SqliteStorage as ExpirationSweeper>::sweep_expired(storage)
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
                    require_finite("warn_mb", warn)?;
                    require_finite("critical_mb", crit)?;
                    let max = req.max_nodes.unwrap_or(10000).min(100_000);
                    let result = storage.check_health(warn, crit, max).await.map_err(|e| {
                        McpError::internal_error(format!("health check failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "consolidate" => {
                    let prune = req.prune_days.unwrap_or(30);
                    if prune < 1 {
                        return Err(McpError::invalid_params("prune_days must be >= 1", None));
                    }
                    let max_sum = req.max_summaries.unwrap_or(50);
                    if max_sum < 1 {
                        return Err(McpError::invalid_params("max_summaries must be >= 1", None));
                    }
                    let result = storage.consolidate(prune, max_sum).await.map_err(|e| {
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
                    require_finite("similarity_threshold", thresh)?;
                    if !(0.0..=1.0).contains(&thresh) {
                        return Err(McpError::invalid_params(
                            "similarity_threshold must be between 0.0 and 1.0",
                            None,
                        ));
                    }
                    let min_cs = req.min_cluster_size.unwrap_or(3);
                    if min_cs < 2 {
                        return Err(McpError::invalid_params(
                            "min_cluster_size must be >= 2",
                            None,
                        ));
                    }
                    let dry = req.dry_run.unwrap_or(false);
                    let result = storage
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
                    let threshold = req.count_threshold.unwrap_or(500).min(10_000);
                    let dry = req.dry_run.unwrap_or(false);
                    let result = storage.auto_compact(threshold, dry).await.map_err(|e| {
                        McpError::internal_error(format!("auto_compact failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "clear_session" => {
                    let sid = req.session_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "session_id is required for lifecycle_action=clear_session",
                            None,
                        )
                    })?;
                    let removed = storage.clear_session(sid).await.map_err(|e| {
                        McpError::internal_error(format!("clear_session failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({"session_id": sid, "removed": removed}).to_string(),
                    )]))
                }
                "backup" => {
                    let info = <SqliteStorage as BackupManager>::create_backup(storage)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("backup failed: {e}"), None)
                        })?;
                    let _ = <SqliteStorage as BackupManager>::rotate_backups(storage, 5).await;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({
                            "path": info.path.display().to_string(),
                            "size_bytes": info.size_bytes,
                            "created_at": info.created_at,
                        })
                        .to_string(),
                    )]))
                }
                "backup_list" => {
                    let backups = <SqliteStorage as BackupManager>::list_backups(storage)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("backup list failed: {e}"), None)
                        })?;
                    let payload: Vec<_> = backups
                        .iter()
                        .map(|b| {
                            json!({
                                "path": b.path.display().to_string(),
                                "size_bytes": b.size_bytes,
                                "created_at": b.created_at,
                            })
                        })
                        .collect();
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "backups": payload, "count": backups.len() }).to_string(),
                    )]))
                }
                other => Err(McpError::invalid_params(
                    format!(
                        "unknown lifecycle_action: {other} (expected sweep|health|consolidate|compact|auto_compact|clear_session|backup|backup_list)"
                    ),
                    None,
                )),
            }
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected update|feedback|relations|lifecycle)"),
            None,
        )),
    }
}

// ── memory_session (unified facade) ──

pub(crate) async fn memory_session(
    storage: &SqliteStorage,
    req: &MemorySessionRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("info");

    match action {
        "info" => match req.info_mode.as_deref().unwrap_or("welcome") {
            "welcome" => {
                let result = storage
                    .welcome(req.session_id.as_deref(), req.project.as_deref())
                    .await
                    .map_err(|e| McpError::internal_error(format!("welcome failed: {e}"), None))?;
                Ok(CallToolResult::success(vec![Content::text(
                    result.to_string(),
                )]))
            }
            "protocol" => {
                let protocol = generate_protocol_markdown();
                Ok(CallToolResult::success(vec![Content::text(protocol)]))
            }
            other => Err(McpError::invalid_params(
                format!("unknown info_mode: {other} (expected welcome|protocol)"),
                None,
            )),
        },
        "checkpoint" => {
            let sub = req.checkpoint_action.as_deref().unwrap_or("save");
            match sub {
                "save" => {
                    let task_title = req.task_title.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "task_title is required for checkpoint_action=save",
                            None,
                        )
                    })?;
                    let progress = req.progress.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "progress is required for checkpoint_action=save",
                            None,
                        )
                    })?;
                    let input = CheckpointInput {
                        task_title: task_title.to_string(),
                        progress: progress.to_string(),
                        plan: req.plan.clone(),
                        files_touched: req.files_touched.clone(),
                        decisions: req.decisions.clone(),
                        key_context: req.key_context.clone(),
                        next_steps: req.next_steps.clone(),
                        session_id: req.session_id.clone(),
                        project: req.project.clone(),
                    };
                    let memory_id =
                        <SqliteStorage as CheckpointManager>::save_checkpoint(storage, input)
                            .await
                            .map_err(|e| {
                                McpError::internal_error(
                                    format!("failed to save checkpoint: {e}"),
                                    None,
                                )
                            })?;
                    let latest = <SqliteStorage as CheckpointManager>::resume_task(
                        storage,
                        task_title,
                        req.project.as_deref(),
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
                    let query = req.task_title.clone().unwrap_or_default();
                    let limit = req.limit.unwrap_or(1).min(MAX_RESULT_LIMIT);
                    let results = <SqliteStorage as CheckpointManager>::resume_task(
                        storage,
                        &query,
                        req.project.as_deref(),
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
                    format!("unknown checkpoint_action: {other} (expected save|resume)"),
                    None,
                )),
            }
        }
        "remind" => {
            let sub = req.remind_action.as_deref().unwrap_or("set");
            match sub {
                "set" => {
                    let text = req.text.as_deref().ok_or_else(|| {
                        McpError::invalid_params("text is required for remind_action=set", None)
                    })?;
                    let duration = req.duration.as_deref().ok_or_else(|| {
                        McpError::invalid_params("duration is required for remind_action=set", None)
                    })?;
                    let result = <SqliteStorage as ReminderManager>::create_reminder(
                        storage,
                        text,
                        duration,
                        req.context.as_deref(),
                        req.session_id.as_deref(),
                        req.project.as_deref(),
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
                        storage,
                        req.status.as_deref(),
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
                    let reminder_id = req.reminder_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "reminder_id is required for remind_action=dismiss",
                            None,
                        )
                    })?;
                    let result =
                        <SqliteStorage as ReminderManager>::dismiss_reminder(storage, reminder_id)
                            .await
                            .map_err(|e| {
                                McpError::internal_error(
                                    format!("failed to dismiss reminder: {e}"),
                                    None,
                                )
                            })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                _ => Err(McpError::invalid_params(
                    "remind_action must be one of: set, list, dismiss",
                    None,
                )),
            }
        }
        "lessons" => {
            let limit = req.limit.unwrap_or(5).min(MAX_RESULT_LIMIT);
            let lessons = <SqliteStorage as LessonQuerier>::query_lessons(
                storage,
                req.task.as_deref(),
                req.project.as_deref(),
                req.exclude_session.as_deref(),
                req.agent_type.as_deref(),
                limit,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("failed to query lessons: {e}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": lessons }).to_string(),
            )]))
        }
        "profile" => {
            let sub = req.profile_action.as_deref().unwrap_or("read");
            match sub {
                "read" => {
                    let profile = <SqliteStorage as ProfileManager>::get_profile(storage)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("failed to read profile: {e}"), None)
                        })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        profile.to_string(),
                    )]))
                }
                "update" => {
                    let updates = req.update.as_ref().ok_or_else(|| {
                        McpError::invalid_params(
                            "update payload is required for profile_action=update",
                            None,
                        )
                    })?;
                    <SqliteStorage as ProfileManager>::set_profile(storage, updates)
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("failed to update profile: {e}"), None)
                        })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "updated": true }).to_string(),
                    )]))
                }
                _ => Err(McpError::invalid_params(
                    "profile_action must be one of: read, update",
                    None,
                )),
            }
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected info|checkpoint|remind|lessons|profile)"),
            None,
        )),
    }
}

// ── memory_admin (unified facade) ──

pub(crate) async fn memory_admin(
    storage: &SqliteStorage,
    req: &MemoryAdminFacadeRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("health");

    match action {
        "list" => {
            if let Some(event_type) = req.list_event_type.as_deref()
                && !is_valid_event_type(event_type)
            {
                return Err(McpError::invalid_params("invalid event_type", None));
            }
            let sort = req.sort.as_deref().unwrap_or("created");
            let limit = req.list_limit.unwrap_or(10).min(MAX_RESULT_LIMIT);
            if let Some(v) = req.importance_min {
                require_finite("importance_min", v)?;
            }
            let opts = SearchOptions {
                event_type: EventType::from_optional(req.list_event_type.as_deref()),
                project: req.project.clone(),
                session_id: req.session_id.clone(),
                include_superseded: req.include_superseded,
                event_after: req.event_after.clone(),
                event_before: req.event_before.clone(),
                importance_min: req.importance_min,
                created_after: req.created_after.clone(),
                created_before: req.created_before.clone(),
                context_tags: req.context_tags.clone(),
                ..Default::default()
            };
            match sort {
                "created" => {
                    let offset = req.offset.unwrap_or(0);
                    let result = storage.list(offset, limit, &opts).await.map_err(|e| {
                        McpError::internal_error(format!("failed to list memories: {e}"), None)
                    })?;
                    let payload = serialize_results(result.memories)?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "results": payload, "total": result.total }).to_string(),
                    )]))
                }
                "recent" => {
                    let results = storage.recent(limit, &opts).await.map_err(|e| {
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
        "health" => {
            let detail = req.detail.as_deref().unwrap_or("basic");
            match detail {
                "basic" => {
                    storage.stats().await.map_err(|e| {
                        McpError::internal_error(format!("storage probe failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        json!({ "status": "healthy" }).to_string(),
                    )]))
                }
                "stats" => {
                    let stats = storage.stats().await.map_err(|e| {
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
                    let result = storage.type_stats().await.map_err(|e| {
                        McpError::internal_error(format!("type_stats failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "sessions" => {
                    let result = storage.session_stats().await.map_err(|e| {
                        McpError::internal_error(format!("session_stats failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "digest" => {
                    let days = req.days.unwrap_or(7).min(365);
                    let result = storage.weekly_digest(days).await.map_err(|e| {
                        McpError::internal_error(format!("weekly_digest failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![Content::text(
                        result.to_string(),
                    )]))
                }
                "access_rate" => {
                    let result = storage.access_rate_stats().await.map_err(|e| {
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
            let export_data = storage
                .export_all()
                .await
                .map_err(|e| McpError::internal_error(format!("failed to export: {e}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(export_data)]))
        }
        "import" => {
            let data = req.data.as_deref().ok_or_else(|| {
                McpError::invalid_params("data is required for action=import", None)
            })?;
            let count = storage
                .import_all(data)
                .await
                .map_err(|e| McpError::internal_error(format!("failed to import: {e}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "imported_memories": count.0, "imported_relationships": count.1 })
                    .to_string(),
            )]))
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected list|health|export|import)"),
            None,
        )),
    }
}
