use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{search, storage};
use crate::mcp::{
    McpMemoryServer,
    request_types::{ListRequest, SearchRequest, StoreRequest},
};
use crate::memory_core::storage::SqliteStorage;

fn request<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("search request should deserialize")
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

async fn store_fixture(server: &McpMemoryServer, value: Value) {
    storage::memory_store(server.runtime.as_ref(), &request::<StoreRequest>(value))
        .await
        .expect("search fixture should store");
}

#[tokio::test]
async fn search_and_list_tools_route_through_the_server_runtime() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    store_fixture(
        &server,
        json!({
            "content": "alpha runtime phrase needle",
            "id": "mcp-search-alpha",
            "tags": ["runtime", "search"],
            "event_type": "decision",
            "project": "mcp-search"
        }),
    )
    .await;
    store_fixture(
        &server,
        json!({
            "content": "beta runtime content",
            "id": "mcp-search-beta",
            "tags": ["runtime"],
            "event_type": "lesson_learned",
            "project": "mcp-search"
        }),
    )
    .await;
    store_fixture(
        &server,
        json!({
            "content": "unrelated memory",
            "id": "mcp-search-other",
            "tags": ["other"],
            "project": "other-project"
        }),
    )
    .await;

    let phrase = search::memory_search(
        server.runtime.as_ref(),
        &request::<SearchRequest>(json!({
            "mode": "phrase",
            "query": "alpha runtime phrase",
            "project": "mcp-search"
        })),
    )
    .await
    .expect("phrase search should succeed");
    let phrase_payload = result_payload(&phrase);
    assert_eq!(
        result_ids(&phrase_payload),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );
    assert_eq!(
        phrase_payload["results"][0]["content"],
        "alpha runtime phrase needle"
    );

    let tagged = search::memory_search(
        server.runtime.as_ref(),
        &request::<SearchRequest>(json!({
            "mode": "tag",
            "tags": ["runtime", "search"],
            "project": "mcp-search"
        })),
    )
    .await
    .expect("tag search should succeed");
    assert_eq!(
        result_ids(&result_payload(&tagged)),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );

    let created = search::memory_list(
        server.runtime.as_ref(),
        &request::<ListRequest>(json!({
            "sort": "created",
            "event_type": "decision",
            "project": "mcp-search",
            "limit": 10
        })),
    )
    .await
    .expect("created-order list should succeed");
    let created_payload = result_payload(&created);
    assert_eq!(created_payload["total"], 1);
    assert_eq!(
        result_ids(&created_payload),
        BTreeSet::from(["mcp-search-alpha".to_string()])
    );

    let recent = search::memory_list(
        server.runtime.as_ref(),
        &request::<ListRequest>(json!({
            "sort": "recent",
            "project": "mcp-search",
            "limit": 10
        })),
    )
    .await
    .expect("recent list should succeed");
    assert_eq!(
        result_ids(&result_payload(&recent)),
        BTreeSet::from([
            "mcp-search-alpha".to_string(),
            "mcp-search-beta".to_string()
        ])
    );
}

#[tokio::test]
async fn search_and_list_tools_preserve_validation_at_the_runtime_boundary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );

    let invalid_event_type = search::memory_search(
        server.runtime.as_ref(),
        &request::<SearchRequest>(json!({
            "mode": "text",
            "query": "anything",
            "event_type": "unknown_event"
        })),
    )
    .await
    .expect_err("invalid event types should remain rejected");
    assert!(
        format!("{invalid_event_type:?}").contains("invalid event_type"),
        "unexpected event-type error: {invalid_event_type:?}"
    );

    let missing_query = search::memory_search(
        server.runtime.as_ref(),
        &request::<SearchRequest>(json!({"mode": "phrase"})),
    )
    .await
    .expect_err("phrase queries should remain required");
    assert!(
        format!("{missing_query:?}").contains("query is required for mode=phrase"),
        "unexpected missing-query error: {missing_query:?}"
    );

    let empty_tags = search::memory_search(
        server.runtime.as_ref(),
        &request::<SearchRequest>(json!({"mode": "tag", "tags": []})),
    )
    .await
    .expect("empty tag searches should remain successful");
    assert_eq!(result_payload(&empty_tags), json!({"results": []}));

    let invalid_sort = search::memory_list(
        server.runtime.as_ref(),
        &request::<ListRequest>(json!({"sort": "ranked"})),
    )
    .await
    .expect_err("unknown list sorts should remain rejected");
    assert!(
        format!("{invalid_sort:?}").contains("unknown sort: ranked (expected created|recent)"),
        "unexpected invalid-sort error: {invalid_sort:?}"
    );
}
