use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

#[tokio::test]
async fn mcp_stdio_lists_tools_and_calls_health() -> Result<(), Box<dyn std::error::Error>> {
    let test_home = std::env::temp_dir().join(format!("romega-mcp-smoke-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_home)?;

    let service = ()
        .serve(TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_romega-memory")).configure(|cmd| {
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

    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_health"),
        "expected memory_health to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_search"),
        "expected memory_search to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_recent"),
        "expected memory_recent to be registered"
    );

    let health_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_health".into(),
            arguments: None,
            task: None,
        }),
    )
    .await??;

    assert!(
        health_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("healthy"))),
        "expected health result to include 'healthy'"
    );

    let store_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store".into(),
            arguments: Some(
                serde_json::json!({ "content": "search needle item" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        store_result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|text| text.text.contains("id"))),
        "expected store result to return id"
    );

    let search_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_search".into(),
            arguments: Some(
                serde_json::json!({ "query": "needle", "limit": 5 })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        search_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("search needle item"))),
        "expected search result to include stored content"
    );

    let recent_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_recent".into(),
            arguments: Some(
                serde_json::json!({ "limit": 5 })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        recent_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("search needle item"))),
        "expected recent result to include stored content"
    );

    service.cancel().await?;
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
