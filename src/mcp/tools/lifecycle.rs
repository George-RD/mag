use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    BackupManager, EventType, ExpirationSweeper, FeedbackRecorder, MaintenanceManager,
    MemoryUpdate, Updater, is_valid_event_type,
};

use super::super::request_types::{FeedbackRequest, LifecycleRequest, UpdateRequest};
use super::super::validation::require_finite;

// ── memory_update ──

pub(crate) async fn memory_update(
    storage: &SqliteStorage,
    req: &UpdateRequest,
) -> Result<CallToolResult, McpError> {
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
    <SqliteStorage as Updater>::update(storage, &req.id, &update)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to update memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({ "id": req.id, "updated": true }).to_string(),
    )]))
}

// ── memory_feedback ──

pub(crate) async fn memory_feedback(
    storage: &SqliteStorage,
    req: &FeedbackRequest,
) -> Result<CallToolResult, McpError> {
    let rating = req.rating.as_str();
    if !matches!(rating, "helpful" | "unhelpful" | "outdated") {
        return Err(McpError::invalid_params("invalid rating", None));
    }

    let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
        storage,
        &req.memory_id,
        rating,
        req.reason.as_deref(),
    )
    .await
    .map_err(|e| McpError::internal_error(format!("failed to record feedback: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({"memory_id": req.memory_id, "feedback": result}).to_string(),
    )]))
}

// ── memory_lifecycle ──

pub(crate) async fn memory_lifecycle(
    storage: &SqliteStorage,
    req: &LifecycleRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("sweep");

    match action {
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
            let result = storage
                .check_health(warn, crit, max)
                .await
                .map_err(|e| McpError::internal_error(format!("health check failed: {e}"), None))?;
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
                .map_err(|e| McpError::internal_error(format!("compaction failed: {e}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(
                result.to_string(),
            )]))
        }
        "auto_compact" => {
            let threshold = req.count_threshold.unwrap_or(500).min(10_000);
            let dry = req.dry_run.unwrap_or(false);
            let result = storage
                .auto_compact(threshold, dry)
                .await
                .map_err(|e| McpError::internal_error(format!("auto_compact failed: {e}"), None))?;
            Ok(CallToolResult::success(vec![Content::text(
                result.to_string(),
            )]))
        }
        "clear_session" => {
            let sid = req.session_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("session_id is required for clear_session", None)
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
                .map_err(|e| McpError::internal_error(format!("backup failed: {e}"), None))?;
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
                .map_err(|e| McpError::internal_error(format!("backup list failed: {e}"), None))?;
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
                "unknown lifecycle action: {other} (expected sweep|health|consolidate|compact|auto_compact|clear_session|backup|backup_list)"
            ),
            None,
        )),
    }
}
