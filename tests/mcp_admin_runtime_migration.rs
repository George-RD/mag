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

const MINIMAL_TOOL_NAMES: &[&str] = &["memory", "memory_admin", "memory_manage", "memory_session"];

fn advertised_tool_names(tools: &rmcp::model::ListToolsResult) -> Vec<String> {
    let mut names: Vec<String> = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    names
}

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
async fn full_mode_routes_memory_admin_through_the_local_runtime_without_protocol_drift()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home =
        std::env::temp_dir().join(format!("mag-mcp-admin-full-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_home)?;

    let mut service = ()
        .serve(TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_mag")).configure(|cmd| {
                cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
                cmd.arg("serve").arg("--mcp-tools").arg("full");
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
    assert_eq!(advertised_tool_names(&tools), FULL_TOOL_NAMES);

    let health = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(serde_json::Map::new()),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_contents(&health), vec![r#"{"status":"healthy"}"#]);

    let invalid_sort = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "list",
                "sort": "ranked"
            }))),
            task: None,
        }),
    )
    .await?
    .expect_err("unknown sort should remain a protocol invalid-params error");
    assert!(
        format!("{invalid_sort:?}").contains("unknown sort: ranked (expected created|recent)"),
        "unexpected protocol invalid-sort error: {invalid_sort:?}"
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn minimal_mode_routes_memory_admin_through_the_same_local_runtime_without_protocol_drift()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home =
        std::env::temp_dir().join(format!("mag-mcp-admin-minimal-{}", uuid::Uuid::new_v4()));
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
    assert_eq!(advertised_tool_names(&tools), MINIMAL_TOOL_NAMES);

    let health = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(serde_json::Map::new()),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_contents(&health), vec![r#"{"status":"healthy"}"#]);

    let missing_import_data = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(arguments(serde_json::json!({"action": "import"}))),
            task: None,
        }),
    )
    .await?
    .expect_err("missing import data should remain a protocol invalid-params error");
    assert!(
        format!("{missing_import_data:?}").contains("data is required for action=import"),
        "unexpected protocol missing-data error: {missing_import_data:?}"
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
