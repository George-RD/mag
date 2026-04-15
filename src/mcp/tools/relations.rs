use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{GraphTraverser, RelationshipQuerier, Retriever, VersionChainQuerier};

use super::super::request_types::RelationsRequest;
use super::super::serialize_results;
use super::super::validation::require_finite;

// ── memory_relations ──

pub(crate) async fn memory_relations(
    storage: &SqliteStorage,
    req: &RelationsRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("list");

    match action {
        "list" => {
            let id = req
                .id
                .as_deref()
                .ok_or_else(|| McpError::invalid_params("id is required for action=list", None))?;
            let rels = storage.get_relationships(id).await.map_err(|e| {
                McpError::internal_error(format!("failed to get relationships: {e}"), None)
            })?;
            let payload: Vec<_> = rels
                .into_iter()
                .map(|r| {
                    json!({
                        "id": r.id,
                        "source_id": r.source_id,
                        "target_id": r.target_id,
                        "rel_type": r.rel_type,
                        "weight": r.weight,
                        "metadata": r.metadata,
                        "created_at": r.created_at
                    })
                })
                .collect();
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "relationships": payload }).to_string(),
            )]))
        }
        "add" => {
            let source_id = req.source_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("source_id is required for action=add", None)
            })?;
            let target_id = req.target_id.as_deref().ok_or_else(|| {
                McpError::invalid_params("target_id is required for action=add", None)
            })?;
            let rel_type = req.rel_type.as_deref().ok_or_else(|| {
                McpError::invalid_params("rel_type is required for action=add", None)
            })?;
            let weight = req.weight.unwrap_or(1.0);
            require_finite("weight", weight)?;
            if !(0.0..=1.0).contains(&weight) {
                return Err(McpError::invalid_params(
                    "weight must be between 0.0 and 1.0",
                    None,
                ));
            }
            let metadata = req
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({}));
            let rel_id = storage
                .add_relationship(source_id, target_id, rel_type, weight, &metadata)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to add relationship: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "id": rel_id, "source_id": source_id, "target_id": target_id, "rel_type": rel_type, "weight": weight, "metadata": metadata }).to_string(),
            )]))
        }
        "traverse" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=traverse", None)
            })?;
            storage.retrieve(id).await.map_err(|e| {
                McpError::internal_error(format!("memory not found for traversal: {e}"), None)
            })?;
            let max_hops = req.max_hops.unwrap_or(2);
            if !(1..=5).contains(&max_hops) {
                return Err(McpError::invalid_params(
                    "max_hops must be between 1 and 5",
                    None,
                ));
            }
            let min_weight = req.min_weight.unwrap_or(0.0);
            require_finite("min_weight", min_weight)?;
            if !(0.0..=1.0).contains(&min_weight) {
                return Err(McpError::invalid_params(
                    "min_weight must be between 0.0 and 1.0",
                    None,
                ));
            }
            let nodes = <SqliteStorage as GraphTraverser>::traverse(
                storage, id, max_hops, min_weight, None,
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to traverse graph: {e}"), None)
            })?;
            let mut grouped = serde_json::Map::new();
            for node in nodes {
                let key = node.hop.to_string();
                let entry = grouped
                    .entry(key)
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let serde_json::Value::Array(items) = entry {
                    items.push(json!({
                        "id": node.id,
                        "content": node.content,
                        "event_type": node.event_type,
                        "metadata": node.metadata,
                        "hop": node.hop,
                        "weight": node.weight,
                        "edge_type": node.edge_type,
                        "created_at": node.created_at
                    }));
                }
            }
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::Value::Object(grouped).to_string(),
            )]))
        }
        "version_chain" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=version_chain", None)
            })?;
            let results = storage.get_version_chain(id).await.map_err(|e| {
                McpError::internal_error(format!("failed to get version chain: {e}"), None)
            })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "chain": payload }).to_string(),
            )]))
        }
        other => Err(McpError::invalid_params(
            format!("unknown relations action: {other} (expected list|add|traverse|version_chain)"),
            None,
        )),
    }
}
