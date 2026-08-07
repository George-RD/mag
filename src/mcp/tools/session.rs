use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::LocalMemoryRuntime;
use crate::memory_core::{CheckpointInput, WelcomeOptions};

use super::super::generate_protocol_markdown;
use super::super::request_types::{
    CheckpointRequest, LessonsRequest, ProfileRequest, RemindRequest, SessionInfoRequest,
};
use super::super::validation::MAX_RESULT_LIMIT;

// ── memory_checkpoint ──

pub(crate) async fn memory_checkpoint(
    runtime: &LocalMemoryRuntime,
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
            let saved = runtime.save_checkpoint_outcome(input).await.map_err(|e| {
                McpError::internal_error(format!("failed to save checkpoint: {e}"), None)
            })?;

            Ok(CallToolResult::success(vec![Content::text(
                json!({
                    "memory_id": saved.memory_id,
                    "checkpoint_number": saved.checkpoint_number
                })
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
    runtime: &LocalMemoryRuntime,
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
                    McpError::internal_error(format!("failed to create reminder: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
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
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": result }).to_string(),
            )]))
        }
        "dismiss" => {
            let reminder_id = req.reminder_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("reminder_id is required for action=dismiss", None)
            })?;
            let result = runtime.dismiss_reminder(reminder_id).await.map_err(|e| {
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
    runtime: &LocalMemoryRuntime,
    req: &LessonsRequest,
) -> Result<CallToolResult, McpError> {
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
        .map_err(|e| McpError::internal_error(format!("failed to query lessons: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({ "results": lessons }).to_string(),
    )]))
}

// ── memory_profile ──

pub(crate) async fn memory_profile(
    runtime: &LocalMemoryRuntime,
    req: &ProfileRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("read");
    match action {
        "read" => {
            let profile = runtime.get_profile().await.map_err(|e| {
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
            runtime.set_profile(updates).await.map_err(|e| {
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
    runtime: &LocalMemoryRuntime,
    req: &SessionInfoRequest,
) -> Result<CallToolResult, McpError> {
    match req.mode.as_deref().unwrap_or("welcome") {
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
