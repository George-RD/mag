use rmcp::ServerHandler;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::storage;
use crate::mcp::{McpMemoryServer, McpToolMode, request_types::MemoryRequest};
use crate::memory_core::storage::SqliteStorage;

fn request<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("memory facade request should deserialize")
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
async fn unified_memory_facade_routes_all_actions_through_the_server_runtime() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let stored = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "content": "runtime-bound raw memory",
            "id": "mcp-memory-runtime",
            "tags": ["runtime"],
            "importance": 0.75,
            "metadata": {"source": "facade"}
        })),
    )
    .await
    .expect("unified memory store should succeed");
    assert_eq!(result_text(&stored), r#"{"id":"mcp-memory-runtime"}"#);

    let retrieved = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "action": "retrieve",
            "id": "mcp-memory-runtime"
        })),
    )
    .await
    .expect("unified memory retrieve should succeed");
    assert_eq!(
        result_text(&retrieved),
        r#"{"content":"runtime-bound raw memory","id":"mcp-memory-runtime"}"#
    );

    let batch = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "action": "store_batch",
            "items": [
                {"content": "first raw batch memory", "id": "mcp-memory-batch-1"},
                {"content": "second raw batch memory", "id": "mcp-memory-batch-2"}
            ]
        })),
    )
    .await
    .expect("unified memory batch store should succeed");
    assert_eq!(
        result_text(&batch),
        r#"{"count":2,"ids":["mcp-memory-batch-1","mcp-memory-batch-2"]}"#
    );

    let batch_retrieved = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "action": "retrieve",
            "id": "mcp-memory-batch-2"
        })),
    )
    .await
    .expect("batch memory should remain visible through the runtime");
    assert_eq!(
        result_text(&batch_retrieved),
        r#"{"content":"second raw batch memory","id":"mcp-memory-batch-2"}"#
    );

    let deleted = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "action": "delete",
            "id": "mcp-memory-runtime"
        })),
    )
    .await
    .expect("unified memory delete should succeed");
    assert_eq!(
        result_text(&deleted),
        r#"{"deleted":true,"id":"mcp-memory-runtime"}"#
    );
}

#[test]
fn minimal_mode_keeps_the_unified_search_tool_available() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    )
    .with_tool_mode(McpToolMode::Minimal);

    assert!(
        server.get_tool("memory_search").is_some(),
        "minimal MCP mode must retain the unified search tool so it can perform MAG's core retrieval workflow"
    );
}

#[tokio::test]
async fn unified_memory_facade_preserves_validation_at_the_runtime_boundary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let missing_content = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({"action": "store"})),
    )
    .await
    .expect_err("store content should remain required");
    assert!(
        format!("{missing_content:?}").contains("content is required for action=store"),
        "unexpected missing-content error: {missing_content:?}"
    );

    let missing_items = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({"action": "store_batch"})),
    )
    .await
    .expect_err("batch items should remain required");
    assert!(
        format!("{missing_items:?}").contains("items is required for action=store_batch"),
        "unexpected missing-items error: {missing_items:?}"
    );

    let invalid_action = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({"action": "archive"})),
    )
    .await
    .expect_err("unknown actions should remain invalid");
    assert!(
        format!("{invalid_action:?}")
            .contains("unknown action: archive (expected store|store_batch|retrieve|delete)"),
        "unexpected invalid-action error: {invalid_action:?}"
    );
}
