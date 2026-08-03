use serde_json::json;

use super::facades;
use crate::mcp::{McpMemoryServer, request_types::MemorySessionRequest};
use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{EventType, MemoryInput, Storage};

fn request(value: serde_json::Value) -> MemorySessionRequest {
    serde_json::from_value(value).expect("session request should deserialize")
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
async fn memory_session_routes_all_subfamilies_through_the_server_runtime() {
    let storage = SqliteStorage::new_in_memory().expect("in-memory storage should initialize");
    let lesson = MemoryInput {
        id: Some("session-runtime-lesson".to_string()),
        content: "reuse the stable session runtime boundary".to_string(),
        event_type: Some(EventType::LessonLearned),
        project: Some("mcp-runtime".to_string()),
        ..MemoryInput::default()
    };
    <SqliteStorage as Storage>::store(
        &storage,
        "session-runtime-lesson",
        "reuse the stable session runtime boundary",
        &lesson,
    )
    .await
    .expect("lesson fixture should store");

    let server = McpMemoryServer::new(storage);

    let welcome = facades::memory_session(server.runtime.as_ref(), &request(json!({})))
        .await
        .expect("default welcome should succeed");
    let welcome_payload: serde_json::Value =
        serde_json::from_str(&result_text(&welcome)).expect("welcome should be JSON");
    assert_eq!(welcome_payload["memory_count"], 1);
    assert!(
        welcome_payload["recent_memories"]
            .as_array()
            .expect("recent_memories should be an array")
            .iter()
            .any(|memory| memory["id"] == "session-runtime-lesson")
    );

    let checkpoint = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "checkpoint",
            "task_title": "migrate session facade",
            "progress": "runtime boundary is under test",
            "project": "mcp-runtime"
        })),
    )
    .await
    .expect("checkpoint save should succeed");
    let checkpoint_payload: serde_json::Value =
        serde_json::from_str(&result_text(&checkpoint)).expect("checkpoint should be JSON");
    assert_eq!(checkpoint_payload["checkpoint_number"], 1);
    assert!(checkpoint_payload["memory_id"].as_str().is_some());

    let resumed = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "checkpoint",
            "checkpoint_action": "resume",
            "task_title": "migrate session facade",
            "project": "mcp-runtime",
            "limit": 1
        })),
    )
    .await
    .expect("checkpoint resume should succeed");
    let resumed_text = result_text(&resumed);
    assert!(resumed_text.contains("migrate session facade"));
    assert!(resumed_text.contains("runtime boundary is under test"));

    let reminder = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "remind",
            "text": "finish the session migration",
            "duration": "1h",
            "project": "mcp-runtime"
        })),
    )
    .await
    .expect("reminder set should succeed");
    let reminder_payload: serde_json::Value =
        serde_json::from_str(&result_text(&reminder)).expect("reminder should be JSON");
    let reminder_id = reminder_payload["reminder_id"]
        .as_str()
        .expect("reminder should return its id");

    let reminders = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "remind",
            "remind_action": "list"
        })),
    )
    .await
    .expect("reminder list should succeed");
    let reminders_payload: serde_json::Value =
        serde_json::from_str(&result_text(&reminders)).expect("reminder list should be JSON");
    assert!(
        reminders_payload["results"]
            .as_array()
            .expect("reminder results should be an array")
            .iter()
            .any(|entry| entry["reminder_id"] == reminder_id)
    );

    let dismissed = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "remind",
            "remind_action": "dismiss",
            "reminder_id": reminder_id
        })),
    )
    .await
    .expect("reminder dismiss should succeed");
    let dismissed_payload: serde_json::Value =
        serde_json::from_str(&result_text(&dismissed)).expect("dismiss result should be JSON");
    assert_eq!(dismissed_payload["status"], "dismissed");

    let lessons = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "lessons",
            "project": "mcp-runtime",
            "limit": 5
        })),
    )
    .await
    .expect("lesson query should succeed");
    let lessons_payload: serde_json::Value =
        serde_json::from_str(&result_text(&lessons)).expect("lessons should be JSON");
    assert!(
        lessons_payload["results"]
            .as_array()
            .expect("lesson results should be an array")
            .iter()
            .any(|entry| entry["lesson_id"] == "session-runtime-lesson")
    );

    let profile_update = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "profile",
            "profile_action": "update",
            "update": {"preferred_editor": "helix"}
        })),
    )
    .await
    .expect("profile update should succeed");
    assert_eq!(result_text(&profile_update), r#"{"updated":true}"#);

    let profile = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({"action": "profile"})),
    )
    .await
    .expect("profile read should succeed");
    let profile_payload: serde_json::Value =
        serde_json::from_str(&result_text(&profile)).expect("profile should be JSON");
    assert_eq!(profile_payload["preferred_editor"], "helix");
}

#[tokio::test]
async fn memory_session_preserves_validation_errors_at_the_runtime_boundary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let missing_title = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({
            "action": "checkpoint",
            "progress": "missing title"
        })),
    )
    .await
    .expect_err("checkpoint title should remain required");
    assert!(
        format!("{missing_title:?}").contains("task_title is required for checkpoint_action=save"),
        "unexpected checkpoint validation error: {missing_title:?}"
    );

    let invalid_mode = facades::memory_session(
        server.runtime.as_ref(),
        &request(json!({"info_mode": "summary"})),
    )
    .await
    .expect_err("unknown info mode should remain invalid");
    assert!(
        format!("{invalid_mode:?}")
            .contains("unknown info_mode: summary (expected welcome|protocol)"),
        "unexpected info-mode validation error: {invalid_mode:?}"
    );
}
