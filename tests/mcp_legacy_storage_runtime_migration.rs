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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_mode_routes_legacy_storage_tools_through_the_local_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home =
        std::env::temp_dir().join(format!("mag-mcp-legacy-storage-{}", uuid::Uuid::new_v4()));
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
                "content": "legacy runtime-bound raw memory",
                "id": "mcp-legacy-runtime",
                "tags": ["legacy", "runtime"],
                "importance": 0.8,
                "metadata": {"source": "stdio"}
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&stored),
        vec![r#"{"id":"mcp-legacy-runtime"}"#]
    );

    let retrieved = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_retrieve".into(),
            arguments: Some(arguments(serde_json::json!({"id": "mcp-legacy-runtime"}))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&retrieved),
        vec![r#"{"content":"legacy runtime-bound raw memory","id":"mcp-legacy-runtime"}"#]
    );

    let batch = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store_batch".into(),
            arguments: Some(arguments(serde_json::json!({
                "items": [
                    {"content": "first legacy batch memory", "id": "mcp-legacy-batch-1"},
                    {"content": "second legacy batch memory", "id": "mcp-legacy-batch-2"}
                ]
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&batch),
        vec![r#"{"count":2,"ids":["mcp-legacy-batch-1","mcp-legacy-batch-2"]}"#]
    );

    let batch_retrieved = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "retrieve",
                "id": "mcp-legacy-batch-2"
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&batch_retrieved),
        vec![r#"{"content":"second legacy batch memory","id":"mcp-legacy-batch-2"}"#]
    );

    let deleted = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_delete".into(),
            arguments: Some(arguments(serde_json::json!({"id": "mcp-legacy-runtime"}))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&deleted),
        vec![r#"{"deleted":true,"id":"mcp-legacy-runtime"}"#]
    );

    let invalid_event_type = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store".into(),
            arguments: Some(arguments(serde_json::json!({
                "content": "invalid event type",
                "event_type": "unknown_event"
            }))),
            task: None,
        }),
    )
    .await?
    .expect_err("legacy store should preserve invalid-params errors");
    assert!(
        format!("{invalid_event_type:?}").contains("invalid event_type"),
        "unexpected protocol event-type error: {invalid_event_type:?}"
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
