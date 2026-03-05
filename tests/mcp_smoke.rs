use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_semantic_search"),
        "expected memory_semantic_search to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_recent"),
        "expected memory_recent to be registered"
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
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "memory_tag_search"),
        "expected memory_tag_search to be registered"
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
            .any(|tool| tool.name == "memory_add_relation"),
        "expected memory_add_relation to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_stats"),
        "expected memory_stats to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_export"),
        "expected memory_export to be registered"
    );
    assert!(
        tools.tools.iter().any(|tool| tool.name == "memory_import"),
        "expected memory_import to be registered"
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

    let semantic_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_semantic_search".into(),
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
        semantic_result
            .content
            .iter()
            .any(|c| c.as_text().is_some_and(|text| text.text.contains("score"))),
        "expected semantic result to include scores"
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

    let tag_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_tag_search".into(),
            arguments: Some(
                serde_json::json!({ "tags": ["alpha"], "limit": 5 })
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

    let stats_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_stats".into(),
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
        stats_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("total_memories"))),
        "expected stats to include total_memories"
    );

    let export_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_export".into(),
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
        export_result.content.iter().any(|c| c
            .as_text()
            .is_some_and(|text| text.text.contains("memories"))),
        "expected export to include memories array"
    );

    // Extract exported JSON string and feed it into memory_import
    let export_json: String = export_result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect();

    let import_result = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_import".into(),
            arguments: Some(
                serde_json::json!({ "data": export_json })
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

    service.cancel().await?;
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
