use serde_json::json;

use super::facades;
use crate::mcp::{
    MINIMAL_TOOL_NAMES, McpMemoryServer, McpToolMode,
    request_types::MemoryAdminFacadeRequest,
};
use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{MemoryInput, Storage};

fn request(value: serde_json::Value) -> MemoryAdminFacadeRequest {
    serde_json::from_value(value).expect("admin request should deserialize")
}

fn result_text(result: &rmcp::model::CallToolResult) -> String {
    assert_eq!(result.content.len(), 1, "expected one text content item");
    result.content[0]
        .as_text()
        .expect("expected text content")
        .text
        .clone()
}

#[tokio::test]
async fn memory_admin_routes_through_server_runtime_without_contract_drift() {
    let storage = SqliteStorage::new_in_memory().expect("in-memory storage should initialize");
    let input = MemoryInput {
        id: Some("admin-runtime-id".to_string()),
        content: "admin runtime content".to_string(),
        ..MemoryInput::default()
    };
    <SqliteStorage as Storage>::store(
        &storage,
        "admin-runtime-id",
        "admin runtime content",
        &input,
    )
    .await
    .expect("fixture should store");

    let server = McpMemoryServer::new(storage);

    let health = facades::memory_admin(server.runtime.as_ref(), &request(json!({})))
        .await
        .expect("default health should succeed");
    assert_eq!(result_text(&health), r#"{"status":"healthy"}"#);

    let list = facades::memory_admin(
        server.runtime.as_ref(),
        &request(json!({"action": "list", "list_limit": 5})),
    )
    .await
    .expect("created-order list should succeed");
    let payload: serde_json::Value =
        serde_json::from_str(&result_text(&list)).expect("list result should be JSON");
    assert_eq!(payload["total"], 1);
    assert_eq!(payload["results"][0]["id"], "admin-runtime-id");
    assert_eq!(
        payload["results"][0]["content"],
        "admin runtime content"
    );

    let invalid_sort = facades::memory_admin(
        server.runtime.as_ref(),
        &request(json!({"action": "list", "sort": "ranked"})),
    )
    .await
    .expect_err("unknown sort should remain an invalid-params error");
    assert!(
        format!("{invalid_sort:?}").contains("unknown sort: ranked (expected created|recent)"),
        "unexpected invalid-sort error: {invalid_sort:?}"
    );

    let missing_import_data = facades::memory_admin(
        server.runtime.as_ref(),
        &request(json!({"action": "import"})),
    )
    .await
    .expect_err("missing import data should remain an invalid-params error");
    assert!(
        format!("{missing_import_data:?}").contains("data is required for action=import"),
        "unexpected missing-data error: {missing_import_data:?}"
    );
}

#[test]
fn memory_admin_remains_available_in_full_and_minimal_tool_modes() {
    let router = McpMemoryServer::tool_router();
    let full_tools = router.list_all();
    assert!(
        full_tools.iter().any(|tool| tool.name == "memory_admin"),
        "full mode must advertise memory_admin"
    );
    assert!(
        MINIMAL_TOOL_NAMES.contains(&"memory_admin"),
        "minimal mode must advertise memory_admin"
    );

    let full = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );
    assert_eq!(full.tool_mode, McpToolMode::Full);
    let minimal = full.with_tool_mode(McpToolMode::Minimal);
    assert_eq!(minimal.tool_mode, McpToolMode::Minimal);
}
