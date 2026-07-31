use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{ExpirationSweeper, FeedbackRecorder, PlaceholderEmbedder};

const LIVE_ID: &str = "admin-write-live";
const EXPIRED_ID: &str = "admin-write-expired";
const RELATIONSHIP_ID: &str = "admin-write-relationship";
const FEEDBACK_REASON: &str = "superseded guidance";

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

async fn storage_at(path: PathBuf) -> SqliteStorage {
    tokio::task::spawn_blocking(move || {
        SqliteStorage::new_with_path(path, Arc::new(PlaceholderEmbedder))
    })
    .await
    .expect("SQLite initialization task should not panic")
    .expect("SQLite storage should initialize")
}

fn import_fixture() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "memories": [
            {
                "id": LIVE_ID,
                "content": "Imported live memory",
                "tags": ["administration", "live"],
                "importance": 0.8,
                "metadata": { "origin": "runtime migration fixture" },
                "content_hash": "live-content-hash",
                "source_type": "import",
                "created_at": "2026-07-31T00:00:00.000Z",
                "event_at": "2026-07-31T00:00:00.000Z",
                "last_accessed_at": "2026-07-31T00:00:00.000Z",
                "access_count": 0,
                "session_id": "admin-write-session",
                "event_type": "user_fact",
                "project": "mag",
                "priority": 2,
                "entity_id": "user:george",
                "agent_type": "cli"
            },
            {
                "id": EXPIRED_ID,
                "content": "Imported expired memory",
                "tags": ["administration", "expired"],
                "importance": 0.3,
                "metadata": { "origin": "runtime migration fixture" },
                "content_hash": "expired-content-hash",
                "source_type": "import",
                "created_at": "2000-01-01T00:00:00.000Z",
                "event_at": "2000-01-01T00:00:00.000Z",
                "last_accessed_at": "2000-01-01T00:00:00.000Z",
                "access_count": 0,
                "session_id": "admin-write-session",
                "event_type": "observation",
                "project": "mag",
                "agent_type": "cli",
                "ttl_seconds": 1
            }
        ],
        "relationships": [
            {
                "id": RELATIONSHIP_ID,
                "source_id": LIVE_ID,
                "target_id": EXPIRED_ID,
                "rel_type": "related_to",
                "weight": 0.7,
                "metadata": { "origin": "runtime migration fixture" },
                "created_at": "2026-07-31T00:00:00.000Z"
            }
        ],
        "user_profile": {
            "timezone": "Asia/Dubai"
        }
    })
}

fn normalize_dynamic_export(value: &mut serde_json::Value) {
    let Some(memories) = value["memories"].as_array_mut() else {
        return;
    };
    for memory in memories {
        let Some(memory) = memory.as_object_mut() else {
            continue;
        };
        memory.remove("last_accessed_at");
        let Some(signals) = memory
            .get_mut("metadata")
            .and_then(|metadata| metadata.get_mut("feedback_signals"))
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for signal in signals {
            if let Some(signal) = signal.as_object_mut() {
                signal.remove("at");
            }
        }
    }
}

#[test]
fn direct_write_admin_commands_route_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact_source.contains("local_runtime.import_all(&data).await?"),
        "import still bypasses the selected local runtime"
    );
    assert!(
        compact_source.contains(
            "local_runtime.record_feedback(memory_id,rating.as_str(),reason.as_deref()).await?"
        ),
        "feedback still bypasses the selected local runtime"
    );
    assert!(
        compact_source.contains("local_runtime.sweep_expired().await?"),
        "sweep still bypasses the selected local runtime"
    );
    assert!(
        main_source.contains("Import completed through local memory runtime"),
        "import does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Feedback recorded through local memory runtime"),
        "feedback does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Expiration sweep completed through local memory runtime"),
        "sweep does not report the selected runtime path"
    );
}

#[test]
fn direct_write_admin_commands_preserve_mutation_and_stdout_contracts() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let import_path = home.path().join("admin-write-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_string_pretty(&import_fixture())?,
    )?;
    let import_path = import_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("import path is not valid UTF-8"))?;

    let import_output = run_cli(home.path(), &["import", import_path])?;
    let import_stdout = String::from_utf8(import_output.stdout)?;
    let import_stderr = String::from_utf8(import_output.stderr)?;
    let expected_import = serde_json::json!({
        "imported_memories": 2,
        "imported_relationships": 1,
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(import_stdout.trim())?,
        expected_import
    );
    assert_eq!(import_stdout, format!("{expected_import}\n"));
    assert!(
        import_stderr.contains("Import completed through local memory runtime"),
        "import did not report the selected runtime path: {import_stderr}"
    );

    for (expected_score, expected_signals, expected_flagged) in
        [(-2, 1, false), (-4, 2, true)]
    {
        let feedback_output = run_cli(
            home.path(),
            &[
                "feedback",
                LIVE_ID,
                "outdated",
                "--reason",
                FEEDBACK_REASON,
            ],
        )?;
        let feedback_stdout = String::from_utf8(feedback_output.stdout)?;
        let feedback_stderr = String::from_utf8(feedback_output.stderr)?;
        let expected_feedback = serde_json::json!({
            "memory_id": LIVE_ID,
            "rating": "outdated",
            "new_score": expected_score,
            "total_signals": expected_signals,
            "flagged": expected_flagged,
        });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(feedback_stdout.trim())?,
            expected_feedback
        );
        assert_eq!(feedback_stdout, format!("{expected_feedback}\n"));
        assert!(
            feedback_stderr.contains("Feedback recorded through local memory runtime"),
            "feedback did not report the selected runtime path: {feedback_stderr}"
        );
    }

    let sweep_output = run_cli(home.path(), &["sweep"])?;
    let sweep_stdout = String::from_utf8(sweep_output.stdout)?;
    let sweep_stderr = String::from_utf8(sweep_output.stderr)?;
    let expected_sweep = serde_json::json!({ "swept_count": 1 });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(sweep_stdout.trim())?,
        expected_sweep
    );
    assert_eq!(sweep_stdout, format!("{expected_sweep}\n"));
    assert!(
        sweep_stderr.contains("Expiration sweep completed through local memory runtime"),
        "sweep did not report the selected runtime path: {sweep_stderr}"
    );

    let export_output = run_cli(home.path(), &["export"])?;
    let export: serde_json::Value = serde_json::from_slice(&export_output.stdout)?;
    let memories = export["memories"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("export omitted memories"))?;
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0]["id"], LIVE_ID);
    assert_eq!(memories[0]["content"], "Imported live memory");
    assert_eq!(memories[0]["metadata"]["feedback_score"], -4);
    assert_eq!(memories[0]["metadata"]["flagged_for_review"], true);
    assert_eq!(
        memories[0]["metadata"]["feedback_signals"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(export["relationships"], serde_json::json!([]));
    assert_eq!(export["user_profile"]["timezone"], "Asia/Dubai");

    Ok(())
}

#[tokio::test]
async fn local_runtime_preserves_direct_write_admin_contracts() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let runtime_storage = storage_at(temp.path().join("runtime.db")).await;
    let direct_storage = storage_at(temp.path().join("direct.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(runtime_storage);
    let import_data = serde_json::to_string(&import_fixture())?;

    let runtime_counts = runtime.import_all(&import_data).await?;
    let direct_counts = direct_storage.import_all(&import_data).await?;
    assert_eq!(runtime_counts, direct_counts);
    assert_eq!(runtime_counts, (2, 1));

    for expected_score in [-2, -4] {
        let runtime_feedback = runtime
            .record_feedback(LIVE_ID, "outdated", Some(FEEDBACK_REASON))
            .await?;
        let direct_feedback = <SqliteStorage as FeedbackRecorder>::record_feedback(
            &direct_storage,
            LIVE_ID,
            "outdated",
            Some(FEEDBACK_REASON),
        )
        .await?;
        assert_eq!(runtime_feedback, direct_feedback);
        assert_eq!(runtime_feedback["new_score"], expected_score);
    }

    let runtime_swept = runtime.sweep_expired().await?;
    let direct_swept =
        <SqliteStorage as ExpirationSweeper>::sweep_expired(&direct_storage).await?;
    assert_eq!(runtime_swept, direct_swept);
    assert_eq!(runtime_swept, 1);

    let mut runtime_export: serde_json::Value =
        serde_json::from_str(&runtime.export_all().await?)?;
    let mut direct_export: serde_json::Value =
        serde_json::from_str(&direct_storage.export_all().await?)?;
    normalize_dynamic_export(&mut runtime_export);
    normalize_dynamic_export(&mut direct_export);
    assert_eq!(runtime_export, direct_export);
    assert_eq!(runtime_export["memories"].as_array().map(Vec::len), Some(1));
    assert_eq!(runtime_export["memories"][0]["id"], LIVE_ID);
    assert_eq!(runtime_export["memories"][0]["metadata"]["feedback_score"], -4);
    assert_eq!(
        runtime_export["memories"][0]["metadata"]["flagged_for_review"],
        true
    );
    assert_eq!(runtime_export["relationships"], serde_json::json!([]));
    assert_eq!(
        runtime_export["user_profile"]["timezone"],
        "Asia/Dubai"
    );

    Ok(())
}
