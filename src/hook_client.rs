use anyhow::{Context, Result};
use serde_json::json;
use tracing::debug;

use crate::cli::HookEvent;
use crate::daemon::DaemonInfo;

/// Calls the MAG daemon HTTP API for the given hook event.
///
/// If the daemon is not running (no `DaemonInfo` on disk, or the recorded PID is
/// stale), this function will attempt to auto-start it and poll `/health` until
/// it becomes responsive.
///
/// On **any** error the function returns `Ok(String::new())` — graceful
/// degradation so that plugin scripts always get valid (possibly empty) output.
pub async fn call_hook(event: &HookEvent) -> Result<String> {
    match call_hook_inner(event).await {
        Ok(body) => Ok(body),
        Err(e) => {
            debug!("hook_client error (gracefully degraded): {e:#}");
            Ok(String::new())
        }
    }
}

/// Inner implementation that is allowed to fail; errors are caught by [`call_hook`].
async fn call_hook_inner(event: &HookEvent) -> Result<String> {
    let info = ensure_daemon_running().await?;

    let base = format!("http://127.0.0.1:{}", info.port);
    let (path, body) = event_to_request(event);
    let url = format!("{base}{path}");

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", info.token))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;

    let text = resp
        .text()
        .await
        .with_context(|| format!("reading response body from {url}"))?;

    Ok(text)
}

/// Returns a running daemon's `DaemonInfo`, auto-starting one if necessary.
async fn ensure_daemon_running() -> Result<DaemonInfo> {
    // Try to read existing daemon info.
    if let Some(info) = DaemonInfo::read()? {
        if !info.is_stale() {
            return Ok(info);
        }
        // Stale PID — clean up and fall through to auto-start.
        DaemonInfo::remove();
    }

    // No daemon running — start one.
    start_daemon()?;

    // Poll /health until the daemon is ready.
    let mut last_err = None;
    for attempt in 0..5 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Re-read daemon info each attempt (the daemon writes it on startup).
        let info = match DaemonInfo::read()? {
            Some(i) => i,
            None => {
                last_err = Some(anyhow::anyhow!(
                    "daemon.json not found after attempt {attempt}"
                ));
                continue;
            }
        };

        let health_url = format!("http://127.0.0.1:{}/health", info.port);
        match reqwest::get(&health_url).await {
            Ok(resp) if resp.status().is_success() => return Ok(info),
            Ok(resp) => {
                last_err = Some(anyhow::anyhow!(
                    "/health returned status {} on attempt {attempt}",
                    resp.status()
                ));
            }
            Err(e) => {
                last_err = Some(
                    anyhow::anyhow!(e).context(format!("/health unreachable on attempt {attempt}")),
                );
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("daemon failed to start after 5 attempts")))
}

/// Spawns the daemon process (`mag serve`) in the background.
fn start_daemon() -> Result<()> {
    let exe = std::env::current_exe().context("resolving current executable path")?;
    std::process::Command::new(exe)
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("spawning daemon process")?;
    Ok(())
}

/// Maps a `HookEvent` variant to an HTTP path and JSON body.
fn event_to_request(event: &HookEvent) -> (&'static str, serde_json::Value) {
    match event {
        HookEvent::SessionStart {
            project,
            budget_tokens,
        } => (
            "/hook/session-start",
            json!({
                "project": project,
                "budget_tokens": budget_tokens,
            }),
        ),
        HookEvent::SessionEnd {
            project,
            session_id,
            summary,
        } => (
            "/hook/session-end",
            json!({
                "project": project,
                "session_id": session_id,
                "summary": summary,
            }),
        ),
        HookEvent::CompactRefresh {
            project,
            budget_tokens,
        } => (
            "/hook/compact-refresh",
            json!({
                "project": project,
                "budget_tokens": budget_tokens,
            }),
        ),
        HookEvent::Search {
            query,
            project,
            limit,
        } => (
            "/hook/search",
            json!({
                "query": query,
                "project": project,
                "limit": limit,
            }),
        ),
        HookEvent::Store {
            content,
            tags,
            importance,
            event_type,
            project,
        } => (
            "/hook/store",
            json!({
                "content": content,
                "tags": tags,
                "importance": importance,
                "event_type": event_type,
                "project": project,
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_to_request_session_start() {
        let event = HookEvent::SessionStart {
            project: Some("test-proj".into()),
            budget_tokens: Some(1000),
        };
        let (path, body) = event_to_request(&event);
        assert_eq!(path, "/hook/session-start");
        assert_eq!(body["project"], "test-proj");
        assert_eq!(body["budget_tokens"], 1000);
    }

    #[test]
    fn event_to_request_session_end() {
        let event = HookEvent::SessionEnd {
            project: Some("proj".into()),
            session_id: "sid-123".into(),
            summary: Some("done".into()),
        };
        let (path, body) = event_to_request(&event);
        assert_eq!(path, "/hook/session-end");
        assert_eq!(body["session_id"], "sid-123");
        assert_eq!(body["summary"], "done");
    }

    #[test]
    fn event_to_request_compact_refresh() {
        let event = HookEvent::CompactRefresh {
            project: Some("proj".into()),
            budget_tokens: Some(500),
        };
        let (path, body) = event_to_request(&event);
        assert_eq!(path, "/hook/compact-refresh");
        assert_eq!(body["budget_tokens"], 500);
    }

    #[test]
    fn event_to_request_search() {
        let event = HookEvent::Search {
            query: "find me".into(),
            project: Some("proj".into()),
            limit: Some(5),
        };
        let (path, body) = event_to_request(&event);
        assert_eq!(path, "/hook/search");
        assert_eq!(body["query"], "find me");
        assert_eq!(body["limit"], 5);
    }

    #[test]
    fn event_to_request_store() {
        let event = HookEvent::Store {
            content: "important note".into(),
            tags: vec!["a".into(), "b".into()],
            importance: 0.8,
            event_type: Some("decision".into()),
            project: Some("proj".into()),
        };
        let (path, body) = event_to_request(&event);
        assert_eq!(path, "/hook/store");
        assert_eq!(body["content"], "important note");
        assert_eq!(body["tags"], json!(["a", "b"]));
    }

    #[test]
    fn event_to_request_store_with_none_fields() {
        let event = HookEvent::Store {
            content: "note".into(),
            tags: vec![],
            importance: 0.5,
            event_type: None,
            project: None,
        };
        let (_, body) = event_to_request(&event);
        assert!(body["event_type"].is_null());
        assert!(body["project"].is_null());
    }
}
