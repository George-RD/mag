// cairn:allow-large-module reason: the three unified facade tool bodies, which is the single responsibility docs/specs/module-decomposition.md assigns to this file
use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, ContentBlock},
};
use serde_json::json;

use crate::LocalMemoryRuntime;
use crate::memory_core::{
    CheckpointInput, EventType, SearchOptions, WelcomeOptions, is_valid_event_type,
};

use super::super::request_types::{
    FeedbackRequest, LifecycleRequest, MemoryAdminFacadeRequest, MemoryManageRequest,
    MemorySessionRequest, RelationsRequest, UpdateRequest,
};
use super::super::validation::{MAX_RESULT_LIMIT, require_finite};
use super::super::{generate_protocol_markdown, serialize_results};
use super::{lifecycle, relations};

// ── memory_manage (unified facade) ──

pub(crate) async fn memory_manage(
    runtime: &LocalMemoryRuntime,
    req: &MemoryManageRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("update");

    match action {
        "update" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=update", None)
            })?;
            let request = UpdateRequest {
                id: id.to_string(),
                content: req.content.clone(),
                tags: req.tags.clone(),
                importance: req.importance,
                metadata: req.metadata.clone(),
                event_type: req.event_type.clone(),
                priority: req.priority,
            };
            lifecycle::memory_update(runtime, &request).await
        }
        "feedback" => {
            let memory_id = req.memory_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("memory_id is required for action=feedback", None)
            })?;
            let rating = req.rating.as_deref().ok_or_else(|| {
                McpError::invalid_params("rating is required for action=feedback", None)
            })?;
            let request = FeedbackRequest {
                memory_id: memory_id.to_string(),
                rating: rating.to_string(),
                reason: req.reason.clone(),
            };
            lifecycle::memory_feedback(runtime, &request).await
        }
        "relations" => {
            let sub = req.relations_action.as_deref().unwrap_or("list");
            match sub {
                "list" => {
                    req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params("id is required for relations_action=list", None)
                    })?;
                }
                "add" => {
                    req.source_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "source_id is required for relations_action=add",
                            None,
                        )
                    })?;
                    req.target_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "target_id is required for relations_action=add",
                            None,
                        )
                    })?;
                    req.rel_type.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "rel_type is required for relations_action=add",
                            None,
                        )
                    })?;
                }
                "traverse" => {
                    req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "id is required for relations_action=traverse",
                            None,
                        )
                    })?;
                }
                "version_chain" => {
                    req.id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "id is required for relations_action=version_chain",
                            None,
                        )
                    })?;
                }
                other => {
                    return Err(McpError::invalid_params(
                        format!(
                            "unknown relations_action: {other} (expected list|add|traverse|version_chain)"
                        ),
                        None,
                    ));
                }
            }

            let request = RelationsRequest {
                action: Some(sub.to_string()),
                id: req.id.clone(),
                source_id: req.source_id.clone(),
                target_id: req.target_id.clone(),
                rel_type: req.rel_type.clone(),
                weight: req.weight,
                metadata: req.metadata.clone(),
                max_hops: req.max_hops,
                min_weight: req.min_weight,
            };
            relations::memory_relations(runtime, &request).await
        }
        "lifecycle" => {
            let sub = req.lifecycle_action.as_deref().unwrap_or("sweep");
            match sub {
                "clear_session" => {
                    req.session_id.as_deref().ok_or_else(|| {
                        McpError::invalid_params(
                            "session_id is required for lifecycle_action=clear_session",
                            None,
                        )
                    })?;
                }
                "sweep" | "health" | "consolidate" | "compact" | "auto_compact" | "fts_rebuild"
                | "backup" | "backup_list" => {}
                other => {
                    return Err(McpError::invalid_params(
                        format!(
                            "unknown lifecycle_action: {other} (expected sweep|health|consolidate|compact|auto_compact|fts_rebuild|clear_session|backup|backup_list)"
                        ),
                        None,
                    ));
                }
            }

            let request = LifecycleRequest {
                action: Some(sub.to_string()),
                warn_mb: req.warn_mb,
                critical_mb: req.critical_mb,
                max_nodes: req.max_nodes,
                prune_days: req.prune_days,
                max_summaries: req.max_summaries,
                event_type: req.event_type.clone(),
                similarity_threshold: req.similarity_threshold,
                min_cluster_size: req.min_cluster_size,
                dry_run: req.dry_run,
                session_id: req.session_id.clone(),
                count_threshold: req.count_threshold,
            };
            lifecycle::memory_lifecycle(runtime, &request).await
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected update|feedback|relations|lifecycle)"),
            None,
        )),
    }
}

// ── memory_session (unified facade) ──

pub(crate) async fn memory_session(
    runtime: &LocalMemoryRuntime,
    req: &MemorySessionRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("info");

    match action {
        "info" => match req.info_mode.as_deref().unwrap_or("welcome") {
            "welcome" => {
                let options = WelcomeOptions {
                    session_id: req.session_id.clone(),
                    project: req.project.clone(),
                    ..WelcomeOptions::default()
                };
                let result = runtime
                    .welcome_scoped(&options)
                    .await
                    .map_err(|e| McpError::internal_error(format!("welcome failed: {e}"), None))?;
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    result.to_string(),
                )]))
            }
            "protocol" => {
                let protocol = generate_protocol_markdown();
                Ok(CallToolResult::success(vec![ContentBlock::text(protocol)]))
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
                    let memory_id = runtime.save_checkpoint(input).await.map_err(|e| {
                        McpError::internal_error(format!("failed to save checkpoint: {e}"), None)
                    })?;
                    let latest = runtime
                        .resume_task(task_title, req.project.as_deref(), 1)
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
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        json!({ "memory_id": memory_id, "checkpoint_number": checkpoint_number })
                            .to_string(),
                    )]))
                }
                "resume" => {
                    let query = req.task_title.clone().unwrap_or_default();
                    let limit = req.limit.unwrap_or(1).min(MAX_RESULT_LIMIT);
                    let results = runtime
                        .resume_task(&query, req.project.as_deref(), limit)
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
                    Ok(CallToolResult::success(vec![ContentBlock::text(markdown)]))
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
                    let result = runtime
                        .create_reminder(
                            text,
                            duration,
                            req.context.as_deref(),
                            req.session_id.as_deref(),
                            req.project.as_deref(),
                        )
                        .await
                        .map_err(|e| {
                            McpError::internal_error(
                                format!("failed to create reminder: {e}"),
                                None,
                            )
                        })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        result.to_string(),
                    )]))
                }
                "list" => {
                    let result = runtime
                        .list_reminders(req.status.as_deref())
                        .await
                        .map_err(|e| {
                            McpError::internal_error(format!("failed to list reminders: {e}"), None)
                        })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
                    let result = runtime.dismiss_reminder(reminder_id).await.map_err(|e| {
                        McpError::internal_error(format!("failed to dismiss reminder: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
            let lessons = runtime
                .query_lessons(
                    req.task.as_deref(),
                    req.project.as_deref(),
                    req.exclude_session.as_deref(),
                    req.agent_type.as_deref(),
                    limit,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to query lessons: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![ContentBlock::text(
                json!({ "results": lessons }).to_string(),
            )]))
        }
        "profile" => {
            let sub = req.profile_action.as_deref().unwrap_or("read");
            match sub {
                "read" => {
                    let profile = runtime.get_profile().await.map_err(|e| {
                        McpError::internal_error(format!("failed to read profile: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
                    runtime.set_profile(updates).await.map_err(|e| {
                        McpError::internal_error(format!("failed to update profile: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
    runtime: &LocalMemoryRuntime,
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
                    let result = runtime.list(offset, limit, &opts).await.map_err(|e| {
                        McpError::internal_error(format!("failed to list memories: {e}"), None)
                    })?;
                    let payload = serialize_results(result.memories)?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        json!({ "results": payload, "total": result.total }).to_string(),
                    )]))
                }
                "recent" => {
                    let results = runtime.recent(limit, &opts).await.map_err(|e| {
                        McpError::internal_error(format!("failed to list recents: {e}"), None)
                    })?;
                    let payload = serialize_results(results)?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
                    runtime.stats().await.map_err(|e| {
                        McpError::internal_error(format!("storage probe failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        json!({ "status": "healthy" }).to_string(),
                    )]))
                }
                "stats" => {
                    let stats = runtime.stats().await.map_err(|e| {
                        McpError::internal_error(format!("failed to get stats: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        serde_json::to_string(&stats).map_err(|e| {
                            McpError::internal_error(
                                format!("failed to serialize stats: {e}"),
                                None,
                            )
                        })?,
                    )]))
                }
                "types" => {
                    let result = runtime.type_stats().await.map_err(|e| {
                        McpError::internal_error(format!("type_stats failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        result.to_string(),
                    )]))
                }
                "sessions" => {
                    let result = runtime.session_stats().await.map_err(|e| {
                        McpError::internal_error(format!("session_stats failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        result.to_string(),
                    )]))
                }
                "digest" => {
                    let days = req.days.unwrap_or(7).min(365);
                    let result = runtime.weekly_digest(days).await.map_err(|e| {
                        McpError::internal_error(format!("weekly_digest failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
                        result.to_string(),
                    )]))
                }
                "access_rate" => {
                    let result = runtime.access_rate_stats().await.map_err(|e| {
                        McpError::internal_error(format!("access_rate_stats failed: {e}"), None)
                    })?;
                    Ok(CallToolResult::success(vec![ContentBlock::text(
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
            let export_data = runtime
                .export_all()
                .await
                .map_err(|e| McpError::internal_error(format!("failed to export: {e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(
                export_data,
            )]))
        }
        "import" => {
            let data = req.data.as_deref().ok_or_else(|| {
                McpError::invalid_params("data is required for action=import", None)
            })?;
            let count = runtime
                .import_all(data)
                .await
                .map_err(|e| McpError::internal_error(format!("failed to import: {e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(
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
