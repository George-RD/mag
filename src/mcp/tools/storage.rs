use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{Deleter, Retriever, Storage};

use super::super::build_memory_input;
use super::super::request_types::{
    DeleteRequest, MemoryRequest, RetrieveRequest, StoreBatchRequest, StoreRequest,
};
use super::super::validation::MAX_BATCH_SIZE;

// ── memory_store ──

pub(crate) async fn memory_store(
    storage: &SqliteStorage,
    req: &StoreRequest,
) -> Result<CallToolResult, McpError> {
    let (id, input) = build_memory_input(req)?;
    <SqliteStorage as Storage>::store(storage, &id, &req.content, &input)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to store memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({ "id": id }).to_string(),
    )]))
}

// ── memory_store_batch ──

pub(crate) async fn memory_store_batch(
    storage: &SqliteStorage,
    req: &StoreBatchRequest,
) -> Result<CallToolResult, McpError> {
    if req.items.len() > MAX_BATCH_SIZE {
        return Err(McpError::invalid_params(
            format!(
                "batch size {} exceeds maximum of {MAX_BATCH_SIZE}",
                req.items.len()
            ),
            None,
        ));
    }
    let mut batch_items = Vec::with_capacity(req.items.len());

    for item in &req.items {
        let (id, input) = build_memory_input(item)?;
        batch_items.push((id, item.content.clone(), input));
    }

    storage
        .store_batch(&batch_items)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to batch store: {e}"), None))?;

    let ids: Vec<&str> = batch_items.iter().map(|(id, _, _)| id.as_str()).collect();
    Ok(CallToolResult::success(vec![Content::text(
        json!({ "ids": ids, "count": ids.len() }).to_string(),
    )]))
}

// ── memory_retrieve ──

pub(crate) async fn memory_retrieve(
    storage: &SqliteStorage,
    req: &RetrieveRequest,
) -> Result<CallToolResult, McpError> {
    let content = storage
        .retrieve(&req.id)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to retrieve memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({ "id": req.id, "content": content }).to_string(),
    )]))
}

// ── memory_delete ──

pub(crate) async fn memory_delete(
    storage: &SqliteStorage,
    req: &DeleteRequest,
) -> Result<CallToolResult, McpError> {
    let deleted = storage
        .delete(&req.id)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to delete memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        json!({ "id": req.id, "deleted": deleted }).to_string(),
    )]))
}

// ── memory (unified facade) ──

pub(crate) async fn memory_facade(
    storage: &SqliteStorage,
    req: &MemoryRequest,
) -> Result<CallToolResult, McpError> {
    let action = req.action.as_deref().unwrap_or("store");

    match action {
        "store" => {
            let content = req.content.as_ref().ok_or_else(|| {
                McpError::invalid_params("content is required for action=store", None)
            })?;
            let store_req = StoreRequest {
                content: content.clone(),
                id: req.id.clone(),
                tags: req.tags.clone(),
                importance: req.importance,
                metadata: req.metadata.clone(),
                event_type: req.event_type.clone(),
                session_id: req.session_id.clone(),
                project: req.project.clone(),
                priority: req.priority,
                entity_id: req.entity_id.clone(),
                agent_type: req.agent_type.clone(),
                ttl_seconds: req.ttl_seconds,
                referenced_date: req.referenced_date.clone(),
            };
            let (id, input) = build_memory_input(&store_req)?;
            <SqliteStorage as Storage>::store(storage, &id, content, &input)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to store memory: {e}"), None)
                })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "id": id }).to_string(),
            )]))
        }
        "store_batch" => {
            let items = req.items.as_ref().ok_or_else(|| {
                McpError::invalid_params("items is required for action=store_batch", None)
            })?;
            if items.len() > MAX_BATCH_SIZE {
                return Err(McpError::invalid_params(
                    format!(
                        "batch size {} exceeds maximum of {MAX_BATCH_SIZE}",
                        items.len()
                    ),
                    None,
                ));
            }
            let mut batch_items = Vec::with_capacity(items.len());
            for item in items {
                let (id, input) = build_memory_input(item)?;
                batch_items.push((id, item.content.clone(), input));
            }
            storage.store_batch(&batch_items).await.map_err(|e| {
                McpError::internal_error(format!("failed to batch store: {e}"), None)
            })?;
            let ids: Vec<&str> = batch_items.iter().map(|(id, _, _)| id.as_str()).collect();
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "ids": ids, "count": ids.len() }).to_string(),
            )]))
        }
        "retrieve" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=retrieve", None)
            })?;
            let content = storage.retrieve(id).await.map_err(|e| {
                McpError::internal_error(format!("failed to retrieve memory: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "id": id, "content": content }).to_string(),
            )]))
        }
        "delete" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=delete", None)
            })?;
            let deleted = storage.delete(id).await.map_err(|e| {
                McpError::internal_error(format!("failed to delete memory: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "id": id, "deleted": deleted }).to_string(),
            )]))
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected store|store_batch|retrieve|delete)"),
            None,
        )),
    }
}
