use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_stdio_lists_tools_and_calls_health() -> Result<(), Box<dyn std::error::Error>> {
    let test_home = std::env::temp_dir().join(format!("mag-mcp-smoke-{}", uuid::Uuid::new_v4()));
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

    // Verify consolidated tool names exist
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_admin"),
        "expected memory_admin to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_search"),
        "expected memory_search to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_delete"),
        "expected memory_delete to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_update"),
        "expected memory_update to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_list"),
        "expected memory_list to be registered"
    );
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_relations"),
        "expected memory_relations to be registered"
    );
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_session_info"),
        "expected memory_session_info to be registered"
    );
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_lifecycle"),
        "expected memory_lifecycle to be registered"
    );
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_checkpoint"),
        "expected memory_checkpoint to be registered"
    );

    // Verify old tool names are gone
    assert!(
        !tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_semantic_search"),
        "memory_semantic_search should be consolidated into memory_search"
    );
    assert!(
        !tools.tools.iter().any(|tool| tool.name == "memory_recent"),
        "memory_recent should be consolidated into memory_list"
    );
    assert!(
        !tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_advanced_search"),
        "memory_advanced_search should be consolidated into memory_search"
    );
    assert!(
        !tools.tools.iter().any(|tool| tool.name == "memory_health"),
        "memory_health should be consolidated into memory_admin"
    );
    assert!(
        !tools.tools.iter().any(|tool| tool.name == "memory_export"),
        "memory_export should be consolidated into memory_admin"
    );
    assert!(
        !tools.tools.iter().any(|tool| tool.name == "memory_welcome"),
        "memory_welcome should be consolidated into memory_session_info"
    );
    assert!(
        !tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_protocol"),
        "memory_protocol should be consolidated into memory_session_info"
    );

    // ─── memory_admin (default action=health, detail=basic) ───
    let health_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(
                serde_json::json!({})
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
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

    // ─── memory_store (3 items) ───
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

    let store2_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store".into(),
            arguments: Some(
                serde_json::json!({ "content": "update target", "id": "test-id-2", "tags": ["alpha", "beta"] })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;
    assert!(
        store2_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("test-id-2"))),
        "expected second store to return test-id-2"
    );

    let store3_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_store".into(),
            arguments: Some(
                serde_json::json!({
                    "content": "important memory",
                    "id": "test-id-3",
                    "importance": 0.95,
                    "metadata": {"source": "test"}
                })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;
    assert!(
        store3_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("test-id-3"))),
        "expected store with importance to return test-id-3"
    );

    // ─── memory_search (text mode, default) ───
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
    assert!(
        search_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("importance"))),
        "expected search result to include importance field"
    );

    // ─── memory_search (semantic mode) ───
    let semantic_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_search".into(),
            arguments: Some(
                serde_json::json!({ "mode": "semantic", "query": "needle", "limit": 5 })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        semantic_result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|text| text.text.contains("score"))),
        "expected semantic result to include scores"
    );
    assert!(
        semantic_result.content.iter().all(|c| c
            .as_text()
            .is_none_or(|text| !text.text.contains("\"abstained\""))),
        "expected default semantic mode to preserve the legacy response shape"
    );

    // ─── memory_list (sort=recent) ───
    let recent_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_list".into(),
            arguments: Some(
                serde_json::json!({ "sort": "recent", "limit": 5 })
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

    // ─── memory_list (sort=created, default) ───
    let list_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_list".into(),
            arguments: Some(
                serde_json::json!({ "limit": 10 })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        list_result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|text| text.text.contains("total"))),
        "expected list result to include total count"
    );

    // ─── memory_search (tag mode) ───
    let tag_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_search".into(),
            arguments: Some(
                serde_json::json!({ "mode": "tag", "tags": ["alpha"], "limit": 5 })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        tag_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("update target"))),
        "expected tag search to find tagged memory"
    );

    // ─── memory_update ───
    let update_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_update".into(),
            arguments: Some(
                serde_json::json!({ "id": "test-id-2", "content": "updated content" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        update_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("updated"))),
        "expected update result to confirm update"
    );

    // ─── memory_delete ───
    let delete_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_delete".into(),
            arguments: Some(
                serde_json::json!({ "id": "test-id-2" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        delete_result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|text| text.text.contains("true"))),
        "expected delete to confirm deletion"
    );

    // ─── memory_admin (action=health, detail=stats) ───
    let stats_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(
                serde_json::json!({ "action": "health", "detail": "stats" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        stats_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("total_memories"))),
        "expected stats to include total_memories"
    );

    // ─── memory_admin (action=export) ───
    let export_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(
                serde_json::json!({ "action": "export" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        export_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("memories"))),
        "expected export to include memories array"
    );

    // Extract exported JSON and feed into memory_admin action=import
    let export_json: String = export_result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect();

    let import_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_admin".into(),
            arguments: Some(
                serde_json::json!({ "action": "import", "data": export_json })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        import_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("imported") || text.text.contains("memories"))),
        "expected import to confirm completion"
    );

    // ─── memory_session_info (protocol) ───
    let protocol_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_session_info".into(),
            arguments: Some(
                serde_json::json!({ "mode": "protocol" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            task: None,
        }),
    )
    .await??;

    assert!(
        protocol_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("memory_admin"))),
        "expected protocol to describe memory_admin"
    );

    let shutdown_result = timeout(
        Duration::from_secs(20),
        service.close_with_timeout(Duration::from_secs(5)),
    )
    .await?;
    assert!(
        shutdown_result?.is_some(),
        "expected MCP child process to shut down before the timeout"
    );
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
