mod mcp_support;

use std::{fs, time::Duration};

use mcp_support::tool_request;
use rmcp::{
    ServiceExt,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

const MINIMAL_TOOL_NAMES: &[&str] = &["memory", "memory_admin", "memory_manage", "memory_session"];

fn text_contents(result: &rmcp::model::CallToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimal_mode_routes_unified_memory_actions_and_errors_through_the_local_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home = std::env::temp_dir().join(format!("mag-mcp-memory-{}", uuid::Uuid::new_v4()));
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

    let stored = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({
                "content": "runtime-bound raw memory",
                "id": "mcp-memory-runtime",
                "tags": ["runtime"],
                "importance": 0.75,
                "metadata": {"source": "stdio"}
            }),
        )),
    )
    .await??;
    assert_eq!(
        text_contents(&stored),
        vec![r#"{"id":"mcp-memory-runtime"}"#]
    );

    let retrieved = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({
                "action": "retrieve",
                "id": "mcp-memory-runtime"
            }),
        )),
    )
    .await??;
    assert_eq!(
        text_contents(&retrieved),
        vec![r#"{"content":"runtime-bound raw memory","id":"mcp-memory-runtime"}"#]
    );

    let batch = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({
                "action": "store_batch",
                "items": [
                    {"content": "first raw batch memory", "id": "mcp-memory-batch-1"},
                    {"content": "second raw batch memory", "id": "mcp-memory-batch-2"}
                ]
            }),
        )),
    )
    .await??;
    assert_eq!(
        text_contents(&batch),
        vec![r#"{"count":2,"ids":["mcp-memory-batch-1","mcp-memory-batch-2"]}"#]
    );

    let batch_retrieved = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({
                "action": "retrieve",
                "id": "mcp-memory-batch-2"
            }),
        )),
    )
    .await??;
    assert_eq!(
        text_contents(&batch_retrieved),
        vec![r#"{"content":"second raw batch memory","id":"mcp-memory-batch-2"}"#]
    );

    let deleted = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({
                "action": "delete",
                "id": "mcp-memory-runtime"
            }),
        )),
    )
    .await??;
    assert_eq!(
        text_contents(&deleted),
        vec![r#"{"deleted":true,"id":"mcp-memory-runtime"}"#]
    );

    let missing_content = timeout(
        Duration::from_secs(20),
        service.call_tool(tool_request(
            "memory",
            serde_json::json!({"action": "store"}),
        )),
    )
    .await?
    .expect_err("store content should remain a protocol invalid-params error");
    assert!(
        format!("{missing_content:?}").contains("content is required for action=store"),
        "unexpected protocol store error: {missing_content:?}"
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
