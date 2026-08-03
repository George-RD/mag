use serde::de::DeserializeOwned;
use serde_json::json;

use super::storage;
use crate::mcp::{
    McpMemoryServer,
    request_types::{DeleteRequest, RetrieveRequest, StoreBatchRequest, StoreRequest},
    validation::MAX_BATCH_SIZE,
};
use crate::memory_core::storage::SqliteStorage;

fn request<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("legacy storage request should deserialize")
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
async fn legacy_storage_tools_route_through_the_server_runtime() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let stored = storage::memory_store(
        server.runtime.as_ref(),
        &request::<StoreRequest>(json!({
            "content": "legacy runtime-bound raw memory",
            "id": "mcp-legacy-runtime",
            "tags": ["legacy", "runtime"],
            "importance": 0.8,
            "metadata": {"source": "legacy-tool"}
        })),
    )
    .await
    .expect("legacy store should succeed");
    assert_eq!(result_text(&stored), r#"{"id":"mcp-legacy-runtime"}"#);

    let retrieved = storage::memory_retrieve(
        server.runtime.as_ref(),
        &request::<RetrieveRequest>(json!({"id": "mcp-legacy-runtime"})),
    )
    .await
    .expect("legacy retrieve should succeed");
    assert_eq!(
        result_text(&retrieved),
        r#"{"content":"legacy runtime-bound raw memory","id":"mcp-legacy-runtime"}"#
    );

    let batch = storage::memory_store_batch(
        server.runtime.as_ref(),
        &request::<StoreBatchRequest>(json!({
            "items": [
                {"content": "first legacy batch memory", "id": "mcp-legacy-batch-1"},
                {"content": "second legacy batch memory", "id": "mcp-legacy-batch-2"}
            ]
        })),
    )
    .await
    .expect("legacy batch store should succeed");
    assert_eq!(
        result_text(&batch),
        r#"{"count":2,"ids":["mcp-legacy-batch-1","mcp-legacy-batch-2"]}"#
    );

    let batch_retrieved = storage::memory_retrieve(
        server.runtime.as_ref(),
        &request::<RetrieveRequest>(json!({"id": "mcp-legacy-batch-2"})),
    )
    .await
    .expect("legacy batch memory should remain visible through the runtime");
    assert_eq!(
        result_text(&batch_retrieved),
        r#"{"content":"second legacy batch memory","id":"mcp-legacy-batch-2"}"#
    );

    let deleted = storage::memory_delete(
        server.runtime.as_ref(),
        &request::<DeleteRequest>(json!({"id": "mcp-legacy-runtime"})),
    )
    .await
    .expect("legacy delete should succeed");
    assert_eq!(
        result_text(&deleted),
        r#"{"deleted":true,"id":"mcp-legacy-runtime"}"#
    );
}

#[tokio::test]
async fn legacy_storage_tools_preserve_validation_at_the_runtime_boundary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let invalid_event_type = storage::memory_store(
        server.runtime.as_ref(),
        &request::<StoreRequest>(json!({
            "content": "invalid event type",
            "event_type": "unknown_event"
        })),
    )
    .await
    .expect_err("legacy store should preserve event-type validation");
    assert!(
        format!("{invalid_event_type:?}").contains("invalid event_type"),
        "unexpected event-type error: {invalid_event_type:?}"
    );

    let oversized_batch_size = MAX_BATCH_SIZE + 1;
    let oversized_items: Vec<serde_json::Value> = (0..oversized_batch_size)
        .map(|index| json!({"content": format!("memory {index}")}))
        .collect();
    let oversized_batch = storage::memory_store_batch(
        server.runtime.as_ref(),
        &request::<StoreBatchRequest>(json!({"items": oversized_items})),
    )
    .await
    .expect_err("legacy batch size limit should remain enforced");
    let expected_error =
        format!("batch size {oversized_batch_size} exceeds maximum of {MAX_BATCH_SIZE}");
    assert!(
        format!("{oversized_batch:?}").contains(&expected_error),
        "unexpected batch-size error: {oversized_batch:?}"
    );
}
