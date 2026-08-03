use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

const FULL_TOOL_NAMES: &[&str] = &[
    "memory",
    "memory_admin",
    "memory_checkpoint",
    "memory_delete",
    "memory_feedback",
    "memory_lessons",
    "memory_lifecycle",
    "memory_list",
    "memory_manage",
    "memory_profile",
    "memory_relations",
    "memory_remind",
    "memory_retrieve",
    "memory_search",
    "memory_session",
    "memory_session_info",
    "memory_store",
    "memory_store_batch",
    "memory_update",
];

fn arguments(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value
        .as_object()
        .expect("tool arguments should be an object")
        .clone()
}

fn text_contents(result: &rmcp::model::CallToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

fn text_json(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    serde_json::from_str(&text_contents(result).join("")).expect("tool result should be JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_mode_preserves_legacy_session_tools_through_the_local_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home = std::env::temp_dir().join(format!(
        "mag-mcp-legacy-session-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&test_home)?;

    let mut service = ()
        .serve(TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_mag")).configure(|cmd| {
                cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
                cmd.arg("serve");
                cmd.env("HOME", &test_home);
                cmd.env("USERPROFILE", &test_home);
            }),
        )?)
        .await?;

    let tools = timeout(
        Duration::from_secs(20),
        service.list_tools(Default::default()),
    )
    .await??;
    let mut tool_names: Vec<String> = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    tool_names.sort();
    assert_eq!(tool_names, FULL_TOOL_NAMES);

    let stored = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store".into(),
            arguments: Some(arguments(serde_json::json!({
                "content": "protocol-visible legacy session lesson",
                "id": "mcp-legacy-session-lesson",
                "event_type": "lesson_learned",
                "project": "mcp-legacy-session"
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&stored),
        vec![r#"{"id":"mcp-legacy-session-lesson"}"#]
    );

    let welcome = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session_info".into(),
            arguments: Some(arguments(serde_json::json!({
                "project": "mcp-legacy-session"
            }))),
            task: None,
        }),
    )
    .await??;
    let welcome_payload = text_json(&welcome);
    assert_eq!(welcome_payload["memory_count"], 1);
    assert!(
        welcome_payload["recent_memories"]
            .as_array()
            .expect("recent_memories should be an array")
            .iter()
            .any(|memory| memory["id"] == "mcp-legacy-session-lesson")
    );

    let protocol = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session_info".into(),
            arguments: Some(arguments(serde_json::json!({"mode": "protocol"}))),
            task: None,
        }),
    )
    .await??;
    let protocol_text = text_contents(&protocol).join("");
    assert!(protocol_text.starts_with("# MAG Protocol\n"));
    assert!(protocol_text.contains("**memory_session_info**"));

    let checkpoint = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_checkpoint".into(),
            arguments: Some(arguments(serde_json::json!({
                "task_title": "migrate legacy session tools",
                "progress": "stdio contract is pinned",
                "project": "mcp-legacy-session"
            }))),
            task: None,
        }),
    )
    .await??;
    let checkpoint_payload = text_json(&checkpoint);
    assert_eq!(checkpoint_payload["checkpoint_number"], 1);
    assert!(checkpoint_payload["memory_id"].as_str().is_some());

    let resumed = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_checkpoint".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "resume",
                "task_title": "migrate legacy session tools",
                "project": "mcp-legacy-session",
                "limit": 1
            }))),
            task: None,
        }),
    )
    .await??;
    let resumed_text = text_contents(&resumed).join("");
    assert!(resumed_text.contains("migrate legacy session tools"));
    assert!(resumed_text.contains("stdio contract is pinned"));

    let reminder = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_remind".into(),
            arguments: Some(arguments(serde_json::json!({
                "text": "finish the legacy session migration",
                "duration": "1h",
                "project": "mcp-legacy-session"
            }))),
            task: None,
        }),
    )
    .await??;
    let reminder_payload = text_json(&reminder);
    let reminder_id = reminder_payload["reminder_id"]
        .as_str()
        .expect("reminder should return its id")
        .to_string();

    let reminders = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_remind".into(),
            arguments: Some(arguments(serde_json::json!({"action": "list"}))),
            task: None,
        }),
    )
    .await??;
    assert!(
        text_json(&reminders)["results"]
            .as_array()
            .expect("reminder results should be an array")
            .iter()
            .any(|entry| entry["reminder_id"] == reminder_id)
    );

    let dismissed = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_remind".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "dismiss",
                "reminder_id": reminder_id
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_json(&dismissed)["status"], "dismissed");

    let lessons = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_lessons".into(),
            arguments: Some(arguments(serde_json::json!({
                "project": "mcp-legacy-session",
                "limit": 5
            }))),
            task: None,
        }),
    )
    .await??;
    assert!(
        text_json(&lessons)["results"]
            .as_array()
            .expect("lesson results should be an array")
            .iter()
            .any(|entry| {
                entry["lesson_id"] == "mcp-legacy-session-lesson"
                    && entry["content"] == "protocol-visible legacy session lesson"
            })
    );

    let profile_update = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_profile".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "update",
                "update": {"preferred_editor": "helix"}
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&profile_update),
        vec![r#"{"updated":true}"#]
    );

    let profile = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_profile".into(),
            arguments: Some(arguments(serde_json::json!({}))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_json(&profile)["preferred_editor"], "helix");

    let missing_title = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_checkpoint".into(),
            arguments: Some(arguments(serde_json::json!({
                "progress": "missing title"
            }))),
            task: None,
        }),
    )
    .await?
    .expect_err("legacy checkpoint title should remain a protocol invalid-params error");
    assert!(
        format!("{missing_title:?}").contains("task_title is required for action=save"),
        "unexpected protocol checkpoint error: {missing_title:?}"
    );

    let invalid_mode = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session_info".into(),
            arguments: Some(arguments(serde_json::json!({"mode": "summary"}))),
            task: None,
        }),
    )
    .await?
    .expect_err("unknown legacy session info mode should remain invalid");
    assert!(
        format!("{invalid_mode:?}")
            .contains("unknown session info mode: summary (expected welcome|protocol)"),
        "unexpected protocol session-info error: {invalid_mode:?}"
    );

    let shutdown = timeout(
        Duration::from_secs(20),
        service.close_with_timeout(Duration::from_secs(5)),
    )
    .await?;
    assert!(shutdown?.is_some());
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
