use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use serde_json::json;

use crate::LocalMemoryRuntime;
use crate::memory_core::{EventType, SearchOptions, is_valid_event_type};

use super::super::request_types::{ListRequest, SearchRequest};
use super::super::serialize_results;
use super::super::validation::{MAX_RESULT_LIMIT, require_finite};

struct SearchFilters<'a> {
    event_type: Option<&'a str>,
    project: Option<&'a str>,
    session_id: Option<&'a str>,
    include_superseded: Option<bool>,
    event_after: Option<&'a str>,
    event_before: Option<&'a str>,
    importance_min: Option<f64>,
    created_after: Option<&'a str>,
    created_before: Option<&'a str>,
    context_tags: Option<&'a [String]>,
    explain: Option<bool>,
}

impl<'a> From<&'a SearchRequest> for SearchFilters<'a> {
    fn from(req: &'a SearchRequest) -> Self {
        Self {
            event_type: req.event_type.as_deref(),
            project: req.project.as_deref(),
            session_id: req.session_id.as_deref(),
            include_superseded: req.include_superseded,
            event_after: req.event_after.as_deref(),
            event_before: req.event_before.as_deref(),
            importance_min: req.importance_min,
            created_after: req.created_after.as_deref(),
            created_before: req.created_before.as_deref(),
            context_tags: req.context_tags.as_deref(),
            explain: req.explain,
        }
    }
}

impl<'a> From<&'a ListRequest> for SearchFilters<'a> {
    fn from(req: &'a ListRequest) -> Self {
        Self {
            event_type: req.event_type.as_deref(),
            project: req.project.as_deref(),
            session_id: req.session_id.as_deref(),
            include_superseded: req.include_superseded,
            event_after: req.event_after.as_deref(),
            event_before: req.event_before.as_deref(),
            importance_min: req.importance_min,
            created_after: req.created_after.as_deref(),
            created_before: req.created_before.as_deref(),
            context_tags: req.context_tags.as_deref(),
            explain: None,
        }
    }
}

fn build_search_options(filters: SearchFilters<'_>) -> Result<SearchOptions, McpError> {
    if let Some(event_type) = filters.event_type
        && !is_valid_event_type(event_type)
    {
        return Err(McpError::invalid_params("invalid event_type", None));
    }

    Ok(SearchOptions {
        event_type: EventType::from_optional(filters.event_type),
        project: filters.project.map(str::to_owned),
        session_id: filters.session_id.map(str::to_owned),
        include_superseded: filters.include_superseded,
        event_after: filters.event_after.map(str::to_owned),
        event_before: filters.event_before.map(str::to_owned),
        importance_min: filters.importance_min,
        created_after: filters.created_after.map(str::to_owned),
        created_before: filters.created_before.map(str::to_owned),
        context_tags: filters.context_tags.map(<[String]>::to_vec),
        explain: filters.explain,
        ..Default::default()
    })
}

fn validate_importance_min(value: Option<f64>) -> Result<(), McpError> {
    if let Some(value) = value {
        require_finite("importance_min", value)?;
    }
    Ok(())
}

// ── memory_search ──

pub(crate) async fn memory_search(
    runtime: &LocalMemoryRuntime,
    req: &SearchRequest,
) -> Result<CallToolResult, McpError> {
    let mode = req.mode.as_deref().unwrap_or("text");
    let limit = req.limit.unwrap_or(10).min(MAX_RESULT_LIMIT);
    let use_advanced = req.advanced.unwrap_or(mode == "text");

    validate_importance_min(req.importance_min)?;

    // "similar" mode doesn't use opts — early-return path
    if mode == "similar" {
        let memory_id = req.memory_id.as_deref().ok_or_else(|| {
            McpError::invalid_params("memory_id is required for mode=similar", None)
        })?;
        let results = runtime.find_similar(memory_id, limit).await.map_err(|e| {
            McpError::internal_error(format!("failed to find similar memories: {e}"), None)
        })?;
        let payload = serialize_results(results)?;
        return Ok(CallToolResult::success(vec![Content::text(
            json!({ "results": payload }).to_string(),
        )]));
    }

    // All other modes share event_type validation and SearchOptions.
    let opts = build_search_options(req.into())?;

    if use_advanced {
        match mode {
            "text" | "semantic" => {
                let query = req.query.as_deref().ok_or_else(|| {
                    McpError::invalid_params(format!("query is required for mode={mode}"), None)
                })?;
                let results = runtime
                    .advanced_search(query, limit, &opts)
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
            let results = runtime.search(query, limit, &opts).await.map_err(|e| {
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
            let results = runtime
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
            let results = runtime
                .phrase_search(query, limit, &opts)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("failed to phrase-search memories: {e}"), None)
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
            let results = runtime.get_by_tags(tags, limit, &opts).await.map_err(|e| {
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
    runtime: &LocalMemoryRuntime,
    req: &ListRequest,
) -> Result<CallToolResult, McpError> {
    let opts = build_search_options(req.into())?;
    let sort = req.sort.as_deref().unwrap_or("created");
    let limit = req.limit.unwrap_or(10).min(MAX_RESULT_LIMIT);
    validate_importance_min(req.importance_min)?;

    match sort {
        "created" => {
            let offset = req.offset.unwrap_or(0);
            let result = runtime.list(offset, limit, &opts).await.map_err(|e| {
                McpError::internal_error(format!("failed to list memories: {e}"), None)
            })?;
            let payload = serialize_results(result.memories)?;
            Ok(CallToolResult::success(vec![Content::text(
                json!({ "results": payload, "total": result.total }).to_string(),
            )]))
        }
        "recent" => {
            let results = runtime.recent(limit, &opts).await.map_err(|e| {
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
