use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{EventType, MemoryInput, PlaceholderEmbedder, StatsProvider};

const CONTENT: &str = "Preserve read-only administration contracts";
const SESSION_ID: &str = "admin-read-session";

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

#[test]
fn read_only_admin_commands_route_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    for call in [
        "local_runtime.stats().await?",
        "local_runtime.export_all().await?",
        "local_runtime.type_stats().await?",
        "local_runtime.session_stats().await?",
        "local_runtime.weekly_digest(*days).await?",
        "local_runtime.access_rate_stats().await?",
    ] {
        assert!(
            compact_source.contains(call),
            "read-only administration still bypasses the selected local runtime: {call}"
        );
    }
    assert!(
        main_source.contains("Stats retrieved through local memory runtime"),
        "stats does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Export completed through local memory runtime"),
        "export does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Extended stats retrieved through local memory runtime"),
        "stats-extended does not report the selected runtime path"
    );
}

#[test]
fn read_only_admin_commands_preserve_stdout_contracts() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let data_root = home.path().join(".mag");
    let database_path = data_root.join("memory.db");

    let stats_output = run_cli(home.path(), &["stats"])?;
    let stats_stdout = String::from_utf8(stats_output.stdout)?;
    let stats_stderr = String::from_utf8(stats_output.stderr)?;
    let expected_stats = serde_json::json!({
        "total_memories": 0,
        "total_relationships": 0,
        "average_importance": 0.0,
        "total_access_count": 0,
        "fts5_indexed": 0,
        "fts5_in_sync": true,
        "paths": {
            "database_path": database_path.display().to_string(),
            "data_root": data_root.display().to_string(),
        },
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(stats_stdout.trim())?,
        expected_stats
    );
    assert_eq!(
        stats_stdout,
        format!("{}\n", serde_json::to_string_pretty(&expected_stats)?)
    );
    assert!(
        stats_stderr.contains("Stats retrieved through local memory runtime"),
        "stats did not report the selected runtime path: {stats_stderr}"
    );

    let export_output = run_cli(home.path(), &["export"])?;
    let export_stdout = String::from_utf8(export_output.stdout)?;
    let export_stderr = String::from_utf8(export_output.stderr)?;
    let expected_export = serde_json::json!({
        "version": 1,
        "memories": [],
        "relationships": [],
        "user_profile": {},
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(export_stdout.trim())?,
        expected_export
    );
    assert_eq!(
        export_stdout,
        format!("{}\n", serde_json::to_string_pretty(&expected_export)?)
    );
    assert!(
        export_stderr.contains("Export completed through local memory runtime"),
        "export did not report the selected runtime path: {export_stderr}"
    );

    let cases = [
        (
            vec!["stats-extended", "--action", "types"],
            serde_json::json!({ "_total": 0 }),
        ),
        (
            vec!["stats-extended", "--action", "sessions"],
            serde_json::json!({ "sessions": [], "total_sessions": 0 }),
        ),
        (
            vec!["stats-extended", "--action", "digest", "--days", "14"],
            serde_json::json!({
                "period_days": 14,
                "total_memories": 0,
                "period_new": 0,
                "session_count": 0,
                "type_breakdown": {},
                "growth_pct": 0.0,
                "prev_period_count": 0,
            }),
        ),
        (
            vec!["stats-extended", "--action", "access-rate"],
            serde_json::json!({
                "total_memories": 0,
                "zero_access_count": 0,
                "never_accessed_pct": 0.0,
                "avg_access_count": 0.0,
                "by_type": [],
                "top_accessed": [],
            }),
        ),
    ];

    for (args, expected) in cases {
        let output = run_cli(home.path(), &args)?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(stdout.trim())?,
            expected,
            "unexpected payload for {args:?}"
        );
        assert_eq!(stdout, format!("{expected}\n"));
        assert!(
            stderr.contains("Extended stats retrieved through local memory runtime"),
            "stats-extended did not report the selected runtime path for {args:?}: {stderr}"
        );
    }

    Ok(())
}

#[tokio::test]
async fn local_runtime_preserves_read_only_admin_contracts() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let storage = storage_at(temp.path().join("memory.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(storage.clone());

    let input = MemoryInput {
        id: Some("admin-read-parity".to_string()),
        content: CONTENT.to_string(),
        event_type: Some(EventType::UserFact),
        session_id: Some(SESSION_ID.to_string()),
        project: Some("mag".to_string()),
        agent_type: Some("cli".to_string()),
        importance: 0.75,
        ..Default::default()
    };
    let memory_id = runtime.store(CONTENT, &input).await?;

    let runtime_stats = runtime.stats().await?;
    let direct_stats = storage.stats().await?;
    assert_eq!(runtime_stats, direct_stats);
    assert_eq!(runtime_stats["total_memories"], 1);
    assert_eq!(runtime_stats["total_relationships"], 0);

    let runtime_export = runtime.export_all().await?;
    let direct_export = storage.export_all().await?;
    assert_eq!(runtime_export, direct_export);
    let export: serde_json::Value = serde_json::from_str(&runtime_export)?;
    assert_eq!(export["version"], 1);
    assert_eq!(export["relationships"], serde_json::json!([]));
    assert_eq!(export["memories"][0]["id"], memory_id);
    assert_eq!(
        export["memories"][0]["content"],
        format!("processed: {CONTENT}")
    );

    let runtime_types = runtime.type_stats().await?;
    let direct_types = <SqliteStorage as StatsProvider>::type_stats(&storage).await?;
    assert_eq!(runtime_types, direct_types);
    assert_eq!(runtime_types["_total"], 1);
    assert_eq!(runtime_types["user_fact"], 1);

    let runtime_sessions = runtime.session_stats().await?;
    let direct_sessions = <SqliteStorage as StatsProvider>::session_stats(&storage).await?;
    assert_eq!(runtime_sessions, direct_sessions);
    assert_eq!(runtime_sessions["total_sessions"], 1);
    assert_eq!(runtime_sessions["sessions"][0]["session_id"], SESSION_ID);
    assert_eq!(runtime_sessions["sessions"][0]["count"], 1);

    let runtime_digest = runtime.weekly_digest(14).await?;
    let direct_digest = <SqliteStorage as StatsProvider>::weekly_digest(&storage, 14).await?;
    assert_eq!(runtime_digest, direct_digest);
    assert_eq!(runtime_digest["period_days"], 14);
    assert_eq!(runtime_digest["total_memories"], 1);
    assert_eq!(runtime_digest["period_new"], 1);

    let runtime_access = runtime.access_rate_stats().await?;
    let direct_access = <SqliteStorage as StatsProvider>::access_rate_stats(&storage).await?;
    assert_eq!(runtime_access, direct_access);
    assert_eq!(runtime_access["total_memories"], 1);
    assert_eq!(runtime_access["zero_access_count"], 1);
    assert_eq!(runtime_access["never_accessed_pct"], 100.0);

    Ok(())
}
