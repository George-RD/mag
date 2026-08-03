use serde::de::DeserializeOwned;
use serde_json::json;

use super::session;
use crate::mcp::{
    McpMemoryServer,
    request_types::{
        CheckpointRequest, LessonsRequest, ProfileRequest, RemindRequest, SessionInfoRequest,
    },
};
use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{EventType, MemoryInput, Storage};

fn request<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("legacy session request should deserialize")
}

fn result_text(result: &rmcp::model::CallToolResult) -> String {
    assert_eq!(result.content.len(), 1, "expected one text content item");
    result.content[0]
        .as_text()
        .expect("expected text content")
        .text
        .clone()
}

#[tokio::test]
async fn legacy_session_tools_route_all_subfamilies_through_the_server_runtime() {
    let storage = SqliteStorage::new_in_memory().expect("in-memory storage should initialize");
    let lesson = MemoryInput {
        id: Some("legacy-session-runtime-lesson".to_string()),
        content: "preserve the legacy session protocol contract".to_string(),
        event_type: Some(EventType::LessonLearned),
        project: Some("mcp-runtime".to_string()),
        ..MemoryInput::default()
    };
    <SqliteStorage as Storage>::store(
        &storage,
        "legacy-session-runtime-lesson",
        "preserve the legacy session protocol contract",
        &lesson,
    )
    .await
    .expect("lesson fixture should store");

    let server = McpMemoryServer::new(storage);

    let welcome = session::memory_session_info(
        server.runtime.as_ref(),
        &request::<SessionInfoRequest>(json!({"project": "mcp-runtime"})),
    )
    .await
    .expect("legacy welcome should succeed");
    let welcome_payload: serde_json::Value =
        serde_json::from_str(&result_text(&welcome)).expect("welcome should be JSON");
    assert_eq!(welcome_payload["memory_count"], 1);
    assert!(
        welcome_payload["recent_memories"]
            .as_array()
            .expect("recent_memories should be an array")
            .iter()
            .any(|memory| memory["id"] == "legacy-session-runtime-lesson")
    );

    let protocol = session::memory_session_info(
        server.runtime.as_ref(),
        &request::<SessionInfoRequest>(json!({"mode": "protocol"})),
    )
    .await
    .expect("legacy protocol response should succeed");
    let protocol_text = result_text(&protocol);
    assert!(protocol_text.starts_with("# MAG Protocol\n"));
    assert!(protocol_text.contains("**memory_session_info**"));

    let checkpoint = session::memory_checkpoint(
        server.runtime.as_ref(),
        &request::<CheckpointRequest>(json!({
            "task_title": "migrate legacy session tools",
            "progress": "runtime boundary is under test",
            "project": "mcp-runtime"
        })),
    )
    .await
    .expect("legacy checkpoint save should succeed");
    let checkpoint_payload: serde_json::Value =
        serde_json::from_str(&result_text(&checkpoint)).expect("checkpoint should be JSON");
    assert_eq!(checkpoint_payload["checkpoint_number"], 1);
    assert!(checkpoint_payload["memory_id"].as_str().is_some());

    let resumed = session::memory_checkpoint(
        server.runtime.as_ref(),
        &request::<CheckpointRequest>(json!({
            "action": "resume",
            "task_title": "migrate legacy session tools",
            "project": "mcp-runtime",
            "limit": 1
        })),
    )
    .await
    .expect("legacy checkpoint resume should succeed");
    let resumed_text = result_text(&resumed);
    assert!(resumed_text.contains("migrate legacy session tools"));
    assert!(resumed_text.contains("runtime boundary is under test"));

    let reminder = session::memory_remind(
        server.runtime.as_ref(),
        &request::<RemindRequest>(json!({
            "text": "finish the legacy session migration",
            "duration": "1h",
            "project": "mcp-runtime"
        })),
    )
    .await
    .expect("legacy reminder set should succeed");
    let reminder_payload: serde_json::Value =
        serde_json::from_str(&result_text(&reminder)).expect("reminder should be JSON");
    let reminder_id = reminder_payload["reminder_id"]
        .as_str()
        .expect("reminder should return its id");

    let reminders = session::memory_remind(
        server.runtime.as_ref(),
        &request::<RemindRequest>(json!({"action": "list"})),
    )
    .await
    .expect("legacy reminder list should succeed");
    let reminders_payload: serde_json::Value =
        serde_json::from_str(&result_text(&reminders)).expect("reminder list should be JSON");
    assert!(
        reminders_payload["results"]
            .as_array()
            .expect("reminder results should be an array")
            .iter()
            .any(|entry| entry["reminder_id"] == reminder_id)
    );

    let dismissed = session::memory_remind(
        server.runtime.as_ref(),
        &request::<RemindRequest>(json!({
            "action": "dismiss",
            "reminder_id": reminder_id
        })),
    )
    .await
    .expect("legacy reminder dismiss should succeed");
    let dismissed_payload: serde_json::Value =
        serde_json::from_str(&result_text(&dismissed)).expect("dismiss result should be JSON");
    assert_eq!(dismissed_payload["status"], "dismissed");

    let lessons = session::memory_lessons(
        server.runtime.as_ref(),
        &request::<LessonsRequest>(json!({
            "project": "mcp-runtime",
            "limit": 5
        })),
    )
    .await
    .expect("legacy lesson query should succeed");
    let lessons_payload: serde_json::Value =
        serde_json::from_str(&result_text(&lessons)).expect("lessons should be JSON");
    assert!(
        lessons_payload["results"]
            .as_array()
            .expect("lesson results should be an array")
            .iter()
            .any(|entry| entry["lesson_id"] == "legacy-session-runtime-lesson")
    );

    let profile_update = session::memory_profile(
        server.runtime.as_ref(),
        &request::<ProfileRequest>(json!({
            "action": "update",
            "update": {"preferred_editor": "helix"}
        })),
    )
    .await
    .expect("legacy profile update should succeed");
    assert_eq!(result_text(&profile_update), r#"{"updated":true}"#);

    let profile = session::memory_profile(
        server.runtime.as_ref(),
        &request::<ProfileRequest>(json!({})),
    )
    .await
    .expect("legacy profile read should succeed");
    let profile_payload: serde_json::Value =
        serde_json::from_str(&result_text(&profile)).expect("profile should be JSON");
    assert_eq!(profile_payload["preferred_editor"], "helix");
}

#[tokio::test]
async fn legacy_session_tools_preserve_validation_errors_at_the_runtime_boundary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let missing_title = session::memory_checkpoint(
        server.runtime.as_ref(),
        &request::<CheckpointRequest>(json!({"progress": "missing title"})),
    )
    .await
    .expect_err("legacy checkpoint title should remain required");
    assert!(
        format!("{missing_title:?}").contains("task_title is required for action=save"),
        "unexpected checkpoint validation error: {missing_title:?}"
    );

    let invalid_mode = session::memory_session_info(
        server.runtime.as_ref(),
        &request::<SessionInfoRequest>(json!({"mode": "summary"})),
    )
    .await
    .expect_err("unknown legacy session info mode should remain invalid");
    assert!(
        format!("{invalid_mode:?}")
            .contains("unknown session info mode: summary (expected welcome|protocol)"),
        "unexpected session-info validation error: {invalid_mode:?}"
    );
}
