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

    service.cancel().await?;
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
