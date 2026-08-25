use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, ContentBlock},
};
use serde_json::json;

use crate::LocalMemoryRuntime;

use super::super::build_memory_input;
use super::super::request_types::{
    DeleteRequest, MemoryRequest, RetrieveRequest, SearchRequest, StoreBatchRequest, StoreRequest,
};
use super::super::validation::MAX_BATCH_SIZE;
use super::search;

async fn execute_store(
    runtime: &LocalMemoryRuntime,
    req: &StoreRequest,
) -> Result<CallToolResult, McpError> {
    let (id, input) = build_memory_input(req)?;
    runtime
        .store_raw(&id, &req.content, &input)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to store memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![ContentBlock::text(
        json!({ "id": id }).to_string(),
    )]))
}

async fn execute_store_batch(
    runtime: &LocalMemoryRuntime,
    items: &[StoreRequest],
) -> Result<CallToolResult, McpError> {
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

    runtime
        .store_batch_raw(&batch_items)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to batch store: {e}"), None))?;

    let ids: Vec<&str> = batch_items.iter().map(|(id, _, _)| id.as_str()).collect();
    Ok(CallToolResult::success(vec![ContentBlock::text(
        json!({ "ids": ids, "count": ids.len() }).to_string(),
    )]))
}

async fn execute_retrieve(
    runtime: &LocalMemoryRuntime,
    id: &str,
) -> Result<CallToolResult, McpError> {
    let content = runtime
        .retrieve(id)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to retrieve memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![ContentBlock::text(
        json!({ "id": id, "content": content }).to_string(),
    )]))
}

async fn execute_delete(
    runtime: &LocalMemoryRuntime,
    id: &str,
) -> Result<CallToolResult, McpError> {
    let deleted = runtime
        .delete(id)
        .await
        .map_err(|e| McpError::internal_error(format!("failed to delete memory: {e}"), None))?;

    Ok(CallToolResult::success(vec![ContentBlock::text(
        json!({ "id": id, "deleted": deleted }).to_string(),
    )]))
}

// ── memory_store ──

pub(crate) async fn memory_store(
    runtime: &LocalMemoryRuntime,
    req: &StoreRequest,
) -> Result<CallToolResult, McpError> {
    execute_store(runtime, req).await
}

// ── memory_store_batch ──

pub(crate) async fn memory_store_batch(
    runtime: &LocalMemoryRuntime,
    req: &StoreBatchRequest,
) -> Result<CallToolResult, McpError> {
    execute_store_batch(runtime, &req.items).await
}

// ── memory_retrieve ──

pub(crate) async fn memory_retrieve(
    runtime: &LocalMemoryRuntime,
    req: &RetrieveRequest,
) -> Result<CallToolResult, McpError> {
    execute_retrieve(runtime, &req.id).await
}

// ── memory_delete ──

pub(crate) async fn memory_delete(
    runtime: &LocalMemoryRuntime,
    req: &DeleteRequest,
) -> Result<CallToolResult, McpError> {
    execute_delete(runtime, &req.id).await
}

// ── memory (unified facade) ──

pub(crate) async fn memory_facade(
    runtime: &LocalMemoryRuntime,
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
            execute_store(runtime, &store_req).await
        }
        "store_batch" => {
            let items = req.items.as_deref().ok_or_else(|| {
                McpError::invalid_params("items is required for action=store_batch", None)
            })?;
            execute_store_batch(runtime, items).await
        }
        "retrieve" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=retrieve", None)
            })?;
            execute_retrieve(runtime, id).await
        }
        "search" => {
            let search_req = SearchRequest::from(req);
            search::memory_search(runtime, &search_req).await
        }
        "delete" => {
            let id = req.id.as_deref().ok_or_else(|| {
                McpError::invalid_params("id is required for action=delete", None)
            })?;
            execute_delete(runtime, id).await
        }
        other => Err(McpError::invalid_params(
            format!("unknown action: {other} (expected store|store_batch|retrieve|search|delete)"),
            None,
        )),
    }
}
