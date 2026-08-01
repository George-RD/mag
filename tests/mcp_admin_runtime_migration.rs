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

fn text_contents(result: &rmcp::model::CallToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_mode_routes_memory_admin_through_the_local_runtime_without_protocol_drift(
) -> Result<(), Box<dyn std::error::Error>> {
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
async fn minimal_mode_routes_memory_admin_through_the_same_local_runtime_without_protocol_drift(
) -> Result<(), Box<dyn std::error::Error>> {
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

    let shutdown = timeout(
        Duration::from_secs(20),
        service.close_with_timeout(Duration::from_secs(5)),
    )
    .await?;
    assert!(shutdown?.is_some());
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
