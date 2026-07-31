use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{EventType, LessonQuerier, MemoryInput, PlaceholderEmbedder};

const PROJECT: &str = "mag";
const SESSION_ID: &str = "lesson-runtime-session";
const CONTENT: &str = "Keep lesson queries behind the local runtime boundary";

fn run_cli(home: &Path, args: &[&str]) -> anyhow::Result<Output> {
    let output = Command::new(env!("CARGO_BIN_EXE_mag"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()?;

    anyhow::ensure!(
        output.status.success(),
        "command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn store_lesson(
    home: &Path,
    content: &str,
    session_id: &str,
    project: &str,
) -> anyhow::Result<String> {
    let output = run_cli(
        home,
        &[
            "ingest",
            content,
            "--event-type",
            "lesson_learned",
            "--session-id",
            session_id,
            "--project",
            project,
            "--agent-type",
            "cli",
        ],
    )?;
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    payload["id"]
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("ingest output omitted lesson id"))
}

async fn storage_at(path: PathBuf) -> SqliteStorage {
    tokio::task::spawn_blocking(move || {
        SqliteStorage::new_with_path(path, Arc::new(PlaceholderEmbedder))
    })
    .await
    .expect("SQLite initialization task should not panic")
    .expect("SQLite storage should initialize")
}

#[test]
fn lessons_command_routes_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact_source.contains("local_runtime.query_lessons("),
        "lessons still bypasses the selected local runtime"
    );
    assert!(
        main_source.contains("Lessons queried through local memory runtime"),
        "lessons does not report the selected runtime path"
    );
}

#[test]
fn lessons_command_preserves_compact_json_contract() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let lesson_id = store_lesson(home.path(), CONTENT, SESSION_ID, PROJECT)?;
    store_lesson(
        home.path(),
        "Keep lesson queries behind the local runtime boundary for another project",
        "other-session",
        "other-project",
    )?;

    let output = run_cli(
        home.path(),
        &[
            "lessons",
            "--task",
            "local runtime boundary",
            "--project",
            PROJECT,
            "--limit",
            "1",
        ],
    )?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
    let results = payload["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("lessons output omitted results"))?;
    anyhow::ensure!(results.len() == 1, "expected one filtered lesson");

    let created_at = results[0]["created_at"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("lesson output omitted created_at"))?;
    chrono::DateTime::parse_from_rfc3339(created_at)?;

    let expected = serde_json::json!({
        "results": [{
            "content": format!("processed: {CONTENT}"),
            "lesson_id": lesson_id,
            "session_id": SESSION_ID,
            "access_count": 0,
            "created_at": created_at,
        }]
    });
    assert_eq!(payload, expected);
    assert_eq!(stdout, format!("{expected}\n"));
    assert!(
        stderr.contains("Lessons queried through local memory runtime"),
        "lessons did not report the selected runtime path: {stderr}"
    );

    Ok(())
}

#[tokio::test]
async fn local_runtime_preserves_lesson_query_contract() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let storage = storage_at(temp.path().join("memory.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(storage.clone());

    let lesson = MemoryInput {
        id: Some("lesson-runtime-parity".to_string()),
        content: CONTENT.to_string(),
        event_type: Some(EventType::LessonLearned),
        session_id: Some(SESSION_ID.to_string()),
        project: Some(PROJECT.to_string()),
        agent_type: Some("cli".to_string()),
        ..Default::default()
    };
    let lesson_id = runtime.store(CONTENT, &lesson).await?;

    let other_project_content = format!("{CONTENT} for another project");
    let other_project = MemoryInput {
        id: Some("lesson-other-project".to_string()),
        content: other_project_content.clone(),
        event_type: Some(EventType::LessonLearned),
        session_id: Some("other-session".to_string()),
        project: Some("other-project".to_string()),
        agent_type: Some("cli".to_string()),
        ..Default::default()
    };
    runtime
        .store(&other_project_content, &other_project)
        .await?;

    let runtime_results = runtime
        .query_lessons(
            Some("local runtime boundary"),
            Some(PROJECT),
            None,
            Some("cli"),
            1,
        )
        .await?;
    let direct_results = storage
        .query_lessons(
            Some("local runtime boundary"),
            Some(PROJECT),
            None,
            Some("cli"),
            1,
        )
        .await?;

    assert_eq!(runtime_results, direct_results);
    assert_eq!(runtime_results.len(), 1);
    assert_eq!(runtime_results[0]["lesson_id"], lesson_id);
    assert_eq!(
        runtime_results[0]["content"],
        format!("processed: {CONTENT}")
    );
    assert_eq!(runtime_results[0]["session_id"], SESSION_ID);
    assert_eq!(runtime_results[0]["access_count"], 0);
    chrono::DateTime::parse_from_rfc3339(
        runtime_results[0]["created_at"]
            .as_str()
            .expect("lesson created_at should be a string"),
    )?;

    assert!(
        runtime
            .query_lessons(
                Some("local runtime boundary"),
                Some(PROJECT),
                Some(SESSION_ID),
                Some("cli"),
                5,
            )
            .await?
            .is_empty()
    );
    assert!(
        runtime
            .query_lessons(None, None, None, None, 0)
            .await?
            .is_empty()
    );

    Ok(())
}
