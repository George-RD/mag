use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{facades, lifecycle, relations};
use crate::mcp::{
    McpMemoryServer,
    request_types::{
        FeedbackRequest, LifecycleRequest, MemoryManageRequest, RelationsRequest, UpdateRequest,
    },
};
use crate::memory_core::MemoryInput;
use crate::memory_core::storage::SqliteStorage;

fn request<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("manage request should deserialize")
}

fn result_text(result: &rmcp::model::CallToolResult) -> String {
    assert_eq!(result.content.len(), 1, "expected one text content item");
    result.content[0]
        .as_text()
        .expect("expected text content")
        .text
        .clone()
}

fn result_json(result: &rmcp::model::CallToolResult) -> Value {
    serde_json::from_str(&result_text(result)).expect("tool result should be JSON")
}

async fn seed_memory(server: &McpMemoryServer, id: &str, content: &str) {
    server
        .runtime
        .store_raw(id, content, &MemoryInput::default())
        .await
        .expect("memory should be stored through the runtime");
}

#[tokio::test]
async fn remaining_legacy_manage_handlers_route_through_the_server_runtime() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );
    seed_memory(&server, "manage-source", "source memory").await;
    seed_memory(&server, "manage-target", "target memory").await;

    let updated = lifecycle::memory_update(
        server.runtime.as_ref(),
        &request::<UpdateRequest>(json!({
            "id": "manage-source",
            "content": "updated source memory",
            "tags": ["runtime"]
        })),
    )
    .await
    .expect("legacy update should succeed through the runtime");
    assert_eq!(
        result_text(&updated),
        r#"{"id":"manage-source","updated":true}"#
    );
    assert_eq!(
        server
            .runtime
            .retrieve("manage-source")
            .await
            .expect("updated memory should remain retrievable"),
        "updated source memory"
    );

    let feedback = lifecycle::memory_feedback(
        server.runtime.as_ref(),
        &request::<FeedbackRequest>(json!({
            "memory_id": "manage-source",
            "rating": "helpful",
            "reason": "runtime parity"
        })),
    )
    .await
    .expect("legacy feedback should succeed through the runtime");
    let feedback = result_json(&feedback);
    assert_eq!(feedback["memory_id"], "manage-source");
    assert_eq!(feedback["feedback"]["rating"], "helpful");
    assert_eq!(feedback["feedback"]["new_score"], 1);

    let added = relations::memory_relations(
        server.runtime.as_ref(),
        &request::<RelationsRequest>(json!({
            "action": "add",
            "source_id": "manage-source",
            "target_id": "manage-target",
            "rel_type": "supports",
            "weight": 0.75,
            "metadata": {"source": "runtime-test"}
        })),
    )
    .await
    .expect("legacy relationship add should succeed through the runtime");
    let added = result_json(&added);
    assert_eq!(added["source_id"], "manage-source");
    assert_eq!(added["target_id"], "manage-target");
    assert_eq!(added["rel_type"], "supports");
    assert_eq!(added["weight"], 0.75);

    let listed = relations::memory_relations(
        server.runtime.as_ref(),
        &request::<RelationsRequest>(json!({
            "action": "list",
            "id": "manage-source"
        })),
    )
    .await
    .expect("legacy relationship list should succeed through the runtime");
    let listed = result_json(&listed);
    let relationships = listed["relationships"]
        .as_array()
        .expect("relationships should be an array");
    let supports = relationships
        .iter()
        .find(|relationship| relationship["rel_type"] == "supports")
        .expect("added supports relationship should be listed");
    assert_eq!(supports["target_id"], "manage-target");

    let traversed = relations::memory_relations(
        server.runtime.as_ref(),
        &request::<RelationsRequest>(json!({
            "action": "traverse",
            "id": "manage-source",
            "max_hops": 2,
            "min_weight": 0.5
        })),
    )
    .await
    .expect("legacy graph traversal should succeed through the runtime");
    let traversed = result_json(&traversed);
    let first_hop = traversed["1"]
        .as_array()
        .expect("first-hop traversal results should be an array");
    let target = first_hop
        .iter()
        .find(|node| node["id"] == "manage-target")
        .expect("related target memory should be traversed");
    assert_eq!(target["content"], "target memory");

    let swept = lifecycle::memory_lifecycle(
        server.runtime.as_ref(),
        &request::<LifecycleRequest>(json!({"action": "sweep"})),
    )
    .await
    .expect("legacy sweep should succeed through the runtime");
    assert_eq!(result_text(&swept), r#"{"swept_count":0}"#);

    let auto_compact = lifecycle::memory_lifecycle(
        server.runtime.as_ref(),
        &request::<LifecycleRequest>(json!({
            "action": "auto_compact",
            "count_threshold": 10,
            "dry_run": true
        })),
    )
    .await
    .expect("legacy auto-compaction should succeed through the runtime");
    let auto_compact = result_json(&auto_compact);
    assert_eq!(auto_compact["triggered"], false);
    assert_eq!(auto_compact["count_threshold"], 10);
}

#[tokio::test]
async fn unified_manage_facade_reuses_runtime_handlers_and_preserves_error_vocabulary() {
    let server = McpMemoryServer::new(
        SqliteStorage::new_in_memory().expect("in-memory storage should initialize"),
    );
    seed_memory(&server, "facade-source", "facade source").await;
    seed_memory(&server, "facade-target", "facade target").await;

    let updated = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "id": "facade-source",
            "content": "facade source updated"
        })),
    )
    .await
    .expect("unified update should succeed through the runtime");
    assert_eq!(
        result_text(&updated),
        r#"{"id":"facade-source","updated":true}"#
    );

    let feedback = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "feedback",
            "memory_id": "facade-source",
            "rating": "helpful"
        })),
    )
    .await
    .expect("unified feedback should succeed through the runtime");
    assert_eq!(result_json(&feedback)["feedback"]["rating"], "helpful");

    let added = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "relations",
            "relations_action": "add",
            "source_id": "facade-source",
            "target_id": "facade-target",
            "rel_type": "supports",
            "weight": 0.8
        })),
    )
    .await
    .expect("unified relationship add should succeed through the runtime");
    assert_eq!(result_json(&added)["target_id"], "facade-target");

    let swept = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "lifecycle",
            "lifecycle_action": "sweep"
        })),
    )
    .await
    .expect("unified lifecycle sweep should succeed through the runtime");
    assert_eq!(result_text(&swept), r#"{"swept_count":0}"#);

    let missing_update_id = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({"content": "missing id"})),
    )
    .await
    .expect_err("unified update id should remain required");
    assert!(
        format!("{missing_update_id:?}").contains("id is required for action=update"),
        "unexpected missing update id error: {missing_update_id:?}"
    );

    let missing_relation_source = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "relations",
            "relations_action": "add"
        })),
    )
    .await
    .expect_err("unified relation source should remain required");
    assert!(
        format!("{missing_relation_source:?}")
            .contains("source_id is required for relations_action=add"),
        "unexpected missing relation source error: {missing_relation_source:?}"
    );

    let missing_lifecycle_session = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "lifecycle",
            "lifecycle_action": "clear_session"
        })),
    )
    .await
    .expect_err("unified clear_session id should remain required");
    assert!(
        format!("{missing_lifecycle_session:?}")
            .contains("session_id is required for lifecycle_action=clear_session"),
        "unexpected missing lifecycle session error: {missing_lifecycle_session:?}"
    );

    let invalid_relation_action = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "relations",
            "relations_action": "merge"
        })),
    )
    .await
    .expect_err("unknown relation actions should remain invalid");
    assert!(
        format!("{invalid_relation_action:?}")
            .contains("unknown relations_action: merge (expected list|add|traverse|version_chain)"),
        "unexpected invalid relation action error: {invalid_relation_action:?}"
    );

    let invalid_lifecycle_action = facades::memory_manage(
        server.runtime.as_ref(),
        &request::<MemoryManageRequest>(json!({
            "action": "lifecycle",
            "lifecycle_action": "archive"
        })),
    )
    .await
    .expect_err("unknown lifecycle actions should remain invalid");
    assert!(
        format!("{invalid_lifecycle_action:?}").contains(
            "unknown lifecycle_action: archive (expected sweep|health|consolidate|compact|auto_compact|fts_rebuild|clear_session|backup|backup_list)"
        ),
        "unexpected invalid lifecycle action error: {invalid_lifecycle_action:?}"
    );
}
