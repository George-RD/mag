use serde::de::DeserializeOwned;
use serde_json::json;

use super::storage;
use crate::mcp::{McpMemoryServer, request_types::MemoryRequest};
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

#[tokio::test]
async fn unified_memory_facade_keeps_search_available_in_minimal_mode() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "content": "facade searchable retrieval evidence",
            "id": "mcp-memory-searchable"
        })),
    )
    .await
    .expect("unified memory store should succeed");

    let searched = storage::memory_facade(
        server.runtime.as_ref(),
        &request::<MemoryRequest>(json!({
            "action": "search",
            "query": "searchable",
            "advanced": false,
            "limit": 5
        })),
    )
    .await
    .expect("minimal memory facade must preserve search capability");

    let payload: serde_json::Value =
        serde_json::from_str(&result_text(&searched)).expect("search result should be JSON");
    let ids: Vec<&str> = payload["results"]
        .as_array()
        .expect("search results should be an array")
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"mcp-memory-searchable"),
        "minimal facade search should return the stored memory: {payload}"
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
        format!("{invalid_action:?}").contains(
            "unknown action: archive (expected store|store_batch|retrieve|search|delete)"
        ),
        "unexpected invalid-action error: {invalid_action:?}"
    );
}
