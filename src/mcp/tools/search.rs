use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    AdvancedSearcher, EventType, Lister, PhraseSearcher, Recents, SearchOptions, Searcher,
    SemanticSearcher, SimilarFinder, Tagger, is_valid_event_type,
};

use super::super::request_types::{ListRequest, SearchRequest};
use super::super::serialize_results;
use super::super::validation::{MAX_RESULT_LIMIT, require_finite};

// ── memory_search ──

pub(crate) async fn memory_search(
    storage: &SqliteStorage,
    req: &SearchRequest,
) -> Result<CallToolResult, McpError> {
    let mode = req.mode.as_deref().unwrap_or("text");
    let limit = req.limit.unwrap_or(10).min(MAX_RESULT_LIMIT);
    let use_advanced = req.advanced.unwrap_or(mode == "text");

    if let Some(v) = req.importance_min {
        require_finite("importance_min", v)?;
    }

    // "similar" mode doesn't use opts — early-return path
    if mode == "similar" {
        let memory_id = req.memory_id.as_deref().ok_or_else(|| {
            McpError::invalid_params("memory_id is required for mode=similar", None)
        })?;
        let results = <SqliteStorage as SimilarFinder>::find_similar(storage, memory_id, limit)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to find similar memories: {e}"), None)
            })?;
        let payload = serialize_results(results)?;
        return Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]));
    }

    // All other modes share event_type validation and SearchOptions
    if let Some(event_type) = req.event_type.as_deref()
        && !is_valid_event_type(event_type)
    {
        return Err(McpError::invalid_params("invalid event_type", None));
    }
    let opts = SearchOptions {
        event_type: EventType::from_optional(req.event_type.as_deref()),
        project: req.project.clone(),
        session_id: req.session_id.clone(),
        include_superseded: req.include_superseded,
        event_after: req.event_after.clone(),
        event_before: req.event_before.clone(),
        importance_min: req.importance_min,
        created_after: req.created_after.clone(),
        created_before: req.created_before.clone(),
        context_tags: req.context_tags.clone(),
        explain: req.explain,
        ..Default::default()
    };

    if use_advanced {
        match mode {
            "text" | "semantic" => {
                let query = req.query.as_deref().ok_or_else(|| {
                    McpError::invalid_params(format!("query is required for mode={mode}"), None)
                })?;
                let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
                    storage, query, limit, &opts,
                )
                .await
                .map_err(|e| {
                    McpError::internal_error(
                        format!("failed to advanced-search memories: {e}"),
                        None,
                    )
                })?;

                let abstained = results.is_empty();
                let result_count = results.len();
                let confidence: f64 = results
                    .iter()
                    .filter_map(|r| r.metadata.get("_text_overlap").and_then(|v| v.as_f64()))
                    .fold(0.0f64, f64::max);
                let payload = serialize_results(results)?;

                let mut response = json!({
                    "results": payload,
                    "result_count": result_count,
                    "abstained": abstained,
                });
                if abstained {
                    response["confidence"] = json!(0.0);
                    response["reason"] = json!(format!(
                        "No results met the relevance threshold (text_overlap < {:.2})",
                        crate::memory_core::ABSTENTION_MIN_TEXT
                    ));
                } else {
                    response["confidence"] = json!(confidence);
                }

                return Ok(CallToolResult::success(vec![Content::text(
                    response.to_string(),
                )]));
            }
            "phrase" | "tag" => {}
            other => {
                return Err(McpError::invalid_params(
                    format!(
                        "unknown search mode: {other} (expected text|semantic|phrase|tag|similar)"
                    ),
                    None,
                ));
            }
        }
    }

    match mode {
        "text" => {
            let query = req
                .query
                .as_deref()
                .ok_or_else(|| McpError::invalid_params("query is required for mode=text", None))?;
            let results = storage.search(query, limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to search memories: {e}"), None)
            })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]))
        }
        "semantic" => {
            let query = req.query.as_deref().ok_or_else(|| {
                McpError::invalid_params("query is required for mode=semantic", None)
            })?;
            let results = storage
                .semantic_search(query, limit, &opts)
                .await
                .map_err(|e| {
                    McpError::internal_error(
                        format!("failed to semantic-search memories: {e}"),
                        None,
                    )
                })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]))
        }
        "phrase" => {
            let query = req.query.as_deref().ok_or_else(|| {
                McpError::invalid_params("query is required for mode=phrase", None)
            })?;
            let results =
                <SqliteStorage as PhraseSearcher>::phrase_search(storage, query, limit, &opts)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(
                            format!("failed to phrase-search memories: {e}"),
                            None,
                        )
                    })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]))
        }
        "tag" => {
            let tags = req
                .tags
                .as_ref()
                .ok_or_else(|| McpError::invalid_params("tags is required for mode=tag", None))?;
            if tags.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    json!({ "results": [] }).to_string(),
                )]));
            }
            let results = storage.get_by_tags(tags, limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to search by tags: {e}"), None)
            })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]))
        }
        other => Err(McpError::invalid_params(
            format!("unknown search mode: {other} (expected text|semantic|phrase|tag|similar)"),
            None,
        )),
    }
}

// ── memory_list ──

pub(crate) async fn memory_list(
    storage: &SqliteStorage,
    req: &ListRequest,
) -> Result<CallToolResult, McpError> {
    if let Some(event_type) = req.event_type.as_deref()
        && !is_valid_event_type(event_type)
    {
        return Err(McpError::invalid_params("invalid event_type", None));
    }
    let sort = req.sort.as_deref().unwrap_or("created");
    let limit = req.limit.unwrap_or(10).min(MAX_RESULT_LIMIT);
    if let Some(v) = req.importance_min {
        require_finite("importance_min", v)?;
    }
    let opts = SearchOptions {
        event_type: EventType::from_optional(req.event_type.as_deref()),
        project: req.project.clone(),
        session_id: req.session_id.clone(),
        include_superseded: req.include_superseded,
        event_after: req.event_after.clone(),
        event_before: req.event_before.clone(),
        importance_min: req.importance_min,
        created_after: req.created_after.clone(),
        created_before: req.created_before.clone(),
        context_tags: req.context_tags.clone(),
        ..Default::default()
    };

    match sort {
        "created" => {
            let offset = req.offset.unwrap_or(0);
            let result = storage.list(offset, limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to list memories: {e}"), None)
            })?;
            let payload = serialize_results(result.memories)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload, "total": result.total }).to_string(),
            )]))
        }
        "recent" => {
            let results = storage.recent(limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to list recents: {e}"), None)
            })?;
            let payload = serialize_results(results)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload }).to_string(),
            )]))
        }
        other => Err(McpError::invalid_params(
            format!("unknown sort: {other} (expected created|recent)"),
            None,
        )),
    }
}
