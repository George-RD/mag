use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    CheckpointInput, CheckpointManager, LessonQuerier, ProfileManager, ReminderManager,
    WelcomeProvider,
};

use super::super::generate_protocol_markdown;
use super::super::request_types::{
    CheckpointRequest, LessonsRequest, ProfileRequest, RemindRequest, SessionInfoRequest,
};
use super::super::validation::MAX_RESULT_LIMIT;

// ── memory_checkpoint ──

pub(crate) async fn memory_checkpoint(
    storage: &SqliteStorage,
    req: &CheckpointRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("save");

    match action {
        "save" => {
            let task_title = req.task_title.as_deref().ok_or_else(|| {
                McpError::invalid_params("task_title is required for action=save", None)
            })?;
            let progress = req.progress.as_deref().ok_or_else(|| {
                McpError::invalid_params("progress is required for action=save", None)
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
            let memory_id = <SqliteStorage as CheckpointManager>::save_checkpoint(storage, input)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to save checkpoint: {e}"), None)
                })?;

            let latest = <SqliteStorage as CheckpointManager>::resume_task(
                storage,
                task_title,
                req.project.as_deref(),
                1,
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to resolve checkpoint number: {e}"), None)
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
            .map_err(|e| McpError::internal_error(format!("failed to resume task: {e}"), None))?;

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
            format!("unknown checkpoint action: {other} (expected save|resume)"),
            None,
        )),
    }
}

// ── memory_remind ──

pub(crate) async fn memory_remind(
    storage: &SqliteStorage,
    req: &RemindRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("set");
    match action {
        "set" => {
            let text = req
                .text
                .as_deref()
                .ok_or_else(|| McpError::invalid_params("text is required for action=set", None))?;
            let duration = req.duration.as_deref().ok_or_else(|| {
                McpError::invalid_params("duration is required for action=set", None)
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
            let result =
                <SqliteStorage as ReminderManager>::list_reminders(storage, req.status.as_deref())
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
                McpError::invalid_params("reminder_id is required for action=dismiss", None)
            })?;
            let result = <SqliteStorage as ReminderManager>::dismiss_reminder(storage, reminder_id)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to dismiss reminder: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                result.to_string(),
            )]))
        }
        _ => Err(McpError::invalid_params(
            "action must be one of: set, list, dismiss",
            None,
        )),
    }
}

// ── memory_lessons ──

pub(crate) async fn memory_lessons(
    storage: &SqliteStorage,
    req: &LessonsRequest,
) -> Result<CallToolResult, McpError> {
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

// ── memory_profile ──

pub(crate) async fn memory_profile(
    storage: &SqliteStorage,
    req: &ProfileRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("read");
    match action {
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
                McpError::invalid_params("update payload is required for action=update", None)
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
            "action must be one of: read, update",
            None,
        )),
    }
}

// ── memory_session_info ──

pub(crate) async fn memory_session_info(
    storage: &SqliteStorage,
    req: &SessionInfoRequest,
) -> Result<CallToolResult, McpError> {
    match req.mode.as_deref().unwrap_or("welcome") {
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
            format!("unknown session info mode: {other} (expected welcome|protocol)"),
            None,
        )),
    }
}
