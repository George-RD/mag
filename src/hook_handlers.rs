use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::memory_core::storage::SqliteStorage;
use crate::memory_core::{
    AdvancedSearcher, LessonQuerier, MemoryInput, ReminderManager, SearchOptions, SemanticResult,
    Storage, WelcomeProvider,
};

// ──────────────────────── App state ────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub storage: SqliteStorage,
}

// ──────────────────────── Request types ────────────────────────

#[derive(Debug, Deserialize)]
pub struct SessionStartReq {
    pub project: Option<String>,
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SessionEndReq {
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompactRefreshReq {
    pub project: Option<String>,
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SearchReq {
    pub query: String,
    pub project: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct StoreReq {
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub event_type: Option<String>,
    pub project: Option<String>,
}

// ──────────────────────── Response helpers ────────────────────────

#[derive(Serialize)]
struct TextResponse {
    text: String,
}

#[derive(Serialize)]
struct StoreResponse {
    id: String,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SemanticResult>,
}

/// Map an `anyhow::Error` into a 500 response with the error message.
fn internal_err(e: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
}

// ──────────────────────── Handlers ────────────────────────

/// `POST /hook/session-start`
///
/// Assembles a welcome briefing by combining `WelcomeProvider::welcome()`,
/// `ReminderManager::list_reminders()`, and `LessonQuerier::query_lessons()`,
/// then truncates to `budget_tokens` (default 2000).
pub async fn session_start(
    State(state): State<AppState>,
    Json(req): Json<SessionStartReq>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let budget = req.budget_tokens.unwrap_or(2000);

    // Gather the three sources concurrently.
    let welcome_fut = state.storage.welcome(None, req.project.as_deref());
    let reminders_fut = state.storage.list_reminders(None);
    let lessons_fut = state
        .storage
        .query_lessons(None, req.project.as_deref(), None, None, 5);

    let (welcome_res, reminders_res, lessons_res) =
        tokio::join!(welcome_fut, reminders_fut, lessons_fut);

    let welcome = welcome_res.map_err(internal_err)?;
    let reminders = reminders_res.map_err(internal_err)?;
    let lessons = lessons_res.map_err(internal_err)?;

    // Format sections.
    let mut output = String::new();
    output.push_str("[MEMORY]\n");
    output.push_str(&welcome.to_string());
    output.push('\n');

    if !reminders.is_empty() {
        output.push_str("\n[REMINDERS]\n");
        for r in &reminders {
            output.push_str(&r.to_string());
            output.push('\n');
        }
    }

    if !lessons.is_empty() {
        output.push_str("\n[LESSONS]\n");
        for l in &lessons {
            output.push_str(&l.to_string());
            output.push('\n');
        }
    }

    // Truncate to budget (byte-level, clamped to a char boundary).
    truncate_to_budget(&mut output, budget);

    Ok(Json(TextResponse { text: output }))
}

/// `POST /hook/session-end`
///
/// Stores the session summary as a `session_summary` event.
pub async fn session_end(
    State(state): State<AppState>,
    Json(req): Json<SessionEndReq>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let summary = req.summary.unwrap_or_default();
    if summary.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "summary is required".to_string()));
    }

    let id = Uuid::new_v4().to_string();
    let mut input = MemoryInput {
        content: summary.clone(),
        id: Some(id.clone()),
        session_id: req.session_id,
        project: req.project,
        ..MemoryInput::default()
    };
    input.apply_event_type_defaults(Some("session_summary"));

    <SqliteStorage as Storage>::store(&state.storage, &id, &summary, &input)
        .await
        .map_err(internal_err)?;

    Ok(Json(StoreResponse { id }))
}

/// `POST /hook/compact-refresh`
///
/// Runs an advanced search scoped to the project and truncates to `budget_tokens`
/// (default 800).
pub async fn compact_refresh(
    State(state): State<AppState>,
    Json(req): Json<CompactRefreshReq>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let budget = req.budget_tokens.unwrap_or(800);

    let opts = SearchOptions {
        project: req.project.clone(),
        ..SearchOptions::default()
    };

    // Use a broad query to surface recent/relevant memories for the project.
    let query = req.project.as_deref().unwrap_or("recent context");

    let results =
        <SqliteStorage as AdvancedSearcher>::advanced_search(&state.storage, query, 20, &opts)
            .await
            .map_err(internal_err)?;

    let mut output = String::new();
    output.push_str("[MEMORY]\n");
    for r in &results {
        output.push_str(&r.content);
        output.push('\n');
    }

    truncate_to_budget(&mut output, budget);

    Ok(Json(TextResponse { text: output }))
}

/// `POST /hook/search`
///
/// Proxies to `AdvancedSearcher::advanced_search()`.
pub async fn search(
    State(state): State<AppState>,
    Json(req): Json<SearchReq>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let limit = req.limit.unwrap_or(10);

    let opts = SearchOptions {
        project: req.project,
        ..SearchOptions::default()
    };

    let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
        &state.storage,
        &req.query,
        limit,
        &opts,
    )
    .await
    .map_err(internal_err)?;

    Ok(Json(SearchResponse { results }))
}

/// `POST /hook/store`
///
/// Creates a `MemoryInput` and stores it.
pub async fn store(
    State(state): State<AppState>,
    Json(req): Json<StoreReq>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let mut input = MemoryInput {
        content: req.content.clone(),
        id: Some(id.clone()),
        tags: req.tags.unwrap_or_default(),
        importance: req.importance.unwrap_or(0.5),
        project: req.project,
        ..MemoryInput::default()
    };
    input.apply_event_type_defaults(req.event_type.as_deref());

    <SqliteStorage as Storage>::store(&state.storage, &id, &req.content, &input)
        .await
        .map_err(internal_err)?;

    Ok(Json(StoreResponse { id }))
}

// ──────────────────────── Router ────────────────────────

pub fn hook_router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/hook/session-start", post(session_start))
        .route("/hook/session-end", post(session_end))
        .route("/hook/compact-refresh", post(compact_refresh))
        .route("/hook/search", post(search))
        .route("/hook/store", post(store))
}

// ──────────────────────── Helpers ────────────────────────

/// Truncates `text` in-place to at most `budget` bytes, respecting UTF-8 char
/// boundaries. Uses a simple byte-budget heuristic (1 token ~ 4 bytes).
fn truncate_to_budget(text: &mut String, budget_tokens: usize) {
    let byte_budget = budget_tokens.saturating_mul(4);
    if text.len() > byte_budget {
        let end = text.floor_char_boundary(byte_budget);
        text.truncate(end);
    }
}

// ──────────────────────── Tests ────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_storage() -> SqliteStorage {
        SqliteStorage::new_in_memory().expect("in-memory storage")
    }

    fn app() -> axum::Router {
        hook_router().with_state(AppState {
            storage: test_storage(),
        })
    }

    #[tokio::test]
    async fn session_start_returns_text() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/session-start")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"budget_tokens": 500}"#))
                    .expect("building test request"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert!(json["text"].is_string());
    }

    #[tokio::test]
    async fn session_end_stores_summary() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/session-end")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"summary": "test summary", "session_id": "s1"}"#,
                    ))
                    .expect("building test request"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert!(json["id"].is_string());
    }

    #[tokio::test]
    async fn session_end_rejects_empty_summary() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/session-end")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))
                    .expect("building test request"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn store_and_search_roundtrip() {
        let state = AppState {
            storage: test_storage(),
        };
        let router = hook_router().with_state(state);

        // Store a memory.
        let store_resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/store")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content": "Rust is great for systems programming", "tags": ["rust"]}"#,
                    ))
                    .expect("building test request"),
            )
            .await
            .expect("store request should succeed");

        assert_eq!(store_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(store_resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert!(json["id"].is_string());

        // Search for the stored memory.
        let search_resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/search")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query": "Rust systems programming"}"#))
                    .expect("building test request"),
            )
            .await
            .expect("search request should succeed");

        assert_eq!(search_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(search_resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert!(json["results"].is_array());
    }

    #[tokio::test]
    async fn compact_refresh_returns_text() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/hook/compact-refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"budget_tokens": 200}"#))
                    .expect("building test request"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert!(json["text"].is_string());
    }

    #[test]
    fn truncate_to_budget_noop_when_under_limit() {
        let mut s = "hello world".to_string();
        truncate_to_budget(&mut s, 100);
        assert_eq!(s, "hello world");
    }

    #[test]
    fn truncate_to_budget_trims_long_text() {
        let mut s = "a".repeat(5000);
        truncate_to_budget(&mut s, 2); // 2 tokens * 4 = 8 bytes
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn truncate_to_budget_respects_char_boundary() {
        // Multi-byte character: "é" is 2 bytes in UTF-8.
        let mut s = "ééééé".to_string(); // 10 bytes
        truncate_to_budget(&mut s, 1); // 1 token * 4 = 4 bytes → 2 chars = 4 bytes
        assert_eq!(s, "éé");
    }
}
