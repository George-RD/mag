use std::{collections::BTreeSet, fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::Value;
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

fn arguments(value: Value) -> serde_json::Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments should be an object")
        .clone()
}

fn result_payload(result: &rmcp::model::CallToolResult) -> Value {
    assert_eq!(result.content.len(), 1, "expected one text content item");
    let text = &result.content[0]
        .as_text()
        .expect("expected text content")
        .text;
    serde_json::from_str(text).expect("tool result should contain JSON")
}

fn result_ids(payload: &Value) -> BTreeSet<String> {
    payload["results"]
        .as_array()
        .expect("results should be an array")
        .iter()
        .map(|item| {
            item["id"]
                .as_str()
                .expect("result should contain an id")
                .to_owned()
        })
        .collect()
}

async fn call_tool(
    service: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    value: Value,
) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
    service
        .call_tool(CallToolRequestParams {
            meta: None,
            name: name.to_owned().into(),
            arguments: Some(arguments(value)),
            task: None,
        })
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_mode_routes_search_and_list_through_the_local_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home =
        std::env::temp_dir().join(format!("mag-mcp-search-runtime-{}", uuid::Uuid::new_v4()));
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

    timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_store",
            serde_json::json!({
                "content": "alpha runtime phrase needle",
                "id": "mcp-search-alpha",
                "tags": ["runtime", "search"],
                "event_type": "decision",
                "project": "mcp-search"
            }),
        ),
    )
    .await??;
    timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_store",
            serde_json::json!({
                "content": "beta runtime content",
                "id": "mcp-search-beta",
                "tags": ["runtime"],
                "event_type": "lesson_learned",
                "project": "mcp-search"
            }),
        ),
    )
    .await??;
    timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_store",
            serde_json::json!({
                "content": "unrelated memory",
                "id": "mcp-search-other",
                "tags": ["other"],
                "project": "other-project"
            }),
        ),
    )
    .await??;

    let phrase = timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_search",
            serde_json::json!({
                "mode": "phrase",
                "query": "alpha runtime phrase",
                "project": "mcp-search"
            }),
        ),
    )
    .await??;
    let phrase_payload = result_payload(&phrase);
    assert_eq!(
        result_ids(&phrase_payload),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );
    assert_eq!(
        phrase_payload["results"][0]["content"],
        "alpha runtime phrase needle"
    );

    let tagged = timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_search",
            serde_json::json!({
                "mode": "tag",
                "tags": ["runtime", "search"],
                "project": "mcp-search"
            }),
        ),
    )
    .await??;
    assert_eq!(
        result_ids(&result_payload(&tagged)),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );

    let created = timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_list",
            serde_json::json!({
                "sort": "created",
                "event_type": "decision",
                "project": "mcp-search",
                "limit": 10
            }),
        ),
    )
    .await??;
    let created_payload = result_payload(&created);
    assert_eq!(created_payload["total"], 1);
    assert_eq!(
        result_ids(&created_payload),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );

    let recent = timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_list",
            serde_json::json!({
                "sort": "recent",
                "project": "mcp-search",
                "limit": 10
            }),
        ),
    )
    .await??;
    assert_eq!(
        result_ids(&result_payload(&recent)),
        BTreeSet::from([
            "mcp-search-alpha".to_string(),
            "mcp-search-beta".to_string()
        ])
    );

    let invalid_sort = timeout(
        Duration::from_secs(20),
        call_tool(
            &service,
            "memory_list",
            serde_json::json!({"sort": "ranked"}),
        ),
    )
    .await?
    .expect_err("unknown list sorts should remain invalid params");
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
