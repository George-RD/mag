use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

const MINIMAL_TOOL_NAMES: &[&str] = &["memory", "memory_admin", "memory_manage", "memory_session"];

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimal_mode_routes_unified_session_state_and_errors_through_the_local_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home =
        std::env::temp_dir().join(format!("mag-mcp-session-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_home)?;

    let mut service = ()
        .serve(TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_mag")).configure(|cmd| {
                cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
                cmd.arg("serve").arg("--mcp-tools").arg("minimal");
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
    assert_eq!(tool_names, MINIMAL_TOOL_NAMES);

    let store = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory".into(),
            arguments: Some(arguments(serde_json::json!({
                "content": "protocol-visible session lesson",
                "id": "mcp-session-lesson",
                "event_type": "lesson_learned",
                "project": "mcp-session"
            }))),
            task: None,
        }),
    )
    .await??;
    assert!(text_contents(&store).iter().any(|text| text.contains("mcp-session-lesson")));

    let welcome = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session".into(),
            arguments: Some(arguments(serde_json::json!({
                "project": "mcp-session"
            }))),
            task: None,
        }),
    )
    .await??;
    let welcome_text = text_contents(&welcome).join("");
    let welcome_payload: serde_json::Value = serde_json::from_str(&welcome_text)?;
    assert_eq!(welcome_payload["memory_count"], 1);
    assert!(welcome_payload["recent_memories"]
        .as_array()
        .expect("recent_memories should be an array")
        .iter()
        .any(|memory| memory["id"] == "mcp-session-lesson"));

    let lessons = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "lessons",
                "project": "mcp-session",
                "limit": 5
            }))),
            task: None,
        }),
    )
    .await??;
    assert!(text_contents(&lessons)
        .iter()
        .any(|text| text.contains("mcp-session-lesson")
            && text.contains("protocol-visible session lesson")));

    let profile_update = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "profile",
                "profile_action": "update",
                "update": {"preferred_editor": "helix"}
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_contents(&profile_update), vec![r#"{"updated":true}"#]);

    let profile = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session".into(),
            arguments: Some(arguments(serde_json::json!({"action": "profile"}))),
            task: None,
        }),
    )
    .await??;
    assert!(text_contents(&profile)
        .iter()
        .any(|text| text.contains(r#""preferred_editor":"helix""#)));

    let missing_title = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "checkpoint",
                "progress": "missing title"
            }))),
            task: None,
        }),
    )
    .await?
    .expect_err("checkpoint title should remain a protocol invalid-params error");
    assert!(
        format!("{missing_title:?}")
            .contains("task_title is required for checkpoint_action=save"),
        "unexpected protocol checkpoint error: {missing_title:?}"
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
