// Public-contract coverage for the remaining CLI maintenance migration.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{BackupManager, Deleter, MaintenanceManager, PlaceholderEmbedder};

const STALE_ID: &str = "maintain-stale";
const TARGET_A_ID: &str = "maintain-target-a";
const TARGET_B_ID: &str = "maintain-target-b";
const TARGET_C_ID: &str = "maintain-target-c";
const KEEP_ID: &str = "maintain-keep";
const CLEAR_SESSION: &str = "maintain-clear-session";

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

fn compact_json(output: Output) -> anyhow::Result<(serde_json::Value, String)> {
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let value: serde_json::Value = serde_json::from_str(stdout.trim())?;
    assert_eq!(stdout, format!("{value}\n"));
    Ok((value, stderr))
}

async fn storage_at(path: PathBuf) -> SqliteStorage {
    tokio::task::spawn_blocking(move || {
        SqliteStorage::new_with_path(path, Arc::new(PlaceholderEmbedder))
    })
    .await
    .expect("SQLite initialization task should not panic")
    .expect("SQLite storage should initialize")
}

fn memory(
    id: &str,
    content: &str,
    created_at: &str,
    session_id: &str,
    event_type: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "content": content,
        "tags": ["maintenance"],
        "importance": 0.6,
        "metadata": { "origin": "maintain runtime migration fixture" },
        "content_hash": format!("{id}-hash"),
        "source_type": "import",
        "created_at": created_at,
        "event_at": created_at,
        "last_accessed_at": created_at,
        "access_count": 0,
        "session_id": session_id,
        "event_type": event_type,
        "project": "mag",
        "agent_type": "cli"
    })
}

fn maintenance_fixture() -> serde_json::Value {
    let retained_at = "2099-01-01T00:00:00.000Z";
    serde_json::json!({
        "version": 1,
        "memories": [
            memory(
                STALE_ID,
                "Stale maintenance observation",
                "2000-01-01T00:00:00.000Z",
                "maintain-stale-session",
                "observation"
            ),
            memory(
                TARGET_A_ID,
                "maintenance guidance alpha beta gamma",
                retained_at,
                CLEAR_SESSION,
                "lesson_learned"
            ),
            memory(
                TARGET_B_ID,
                "maintenance guidance alpha beta gamma delta",
                retained_at,
                CLEAR_SESSION,
                "lesson_learned"
            ),
            memory(
                TARGET_C_ID,
                "maintenance guidance alpha beta gamma epsilon",
                retained_at,
                CLEAR_SESSION,
                "lesson_learned"
            ),
            memory(
                KEEP_ID,
                "Retained maintenance control memory",
                retained_at,
                "maintain-keep-session",
                "user_fact"
            )
        ],
        "relationships": [
            {
                "id": "maintain-stale-edge",
                "source_id": STALE_ID,
                "target_id": KEEP_ID,
                "rel_type": "related_to",
                "weight": 0.7,
                "metadata": { "origin": "maintain runtime migration fixture" },
                "created_at": retained_at
            }
        ],
        "user_profile": {
            "timezone": "Asia/Dubai"
        }
    })
}

fn write_fixture(home: &Path) -> anyhow::Result<String> {
    let path = home.join("maintain-import.json");
    std::fs::write(&path, serde_json::to_string_pretty(&maintenance_fixture())?)?;
    Ok(path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("fixture path is not valid UTF-8"))?
        .to_string())
}

#[test]
fn maintain_entrypoints_route_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    for expected_call in [
        "local_runtime.maybe_startup_backup().await",
        "local_runtime.check_health(",
        "local_runtime.consolidate(",
        "local_runtime.compact(",
        "local_runtime.clear_session(sid).await?",
        "local_runtime.rebuild_fts().await?",
        "local_runtime.create_backup().await?",
        "local_runtime.rotate_backups(5).await?",
        "local_runtime.list_backups().await?",
        "local_runtime.restore_backup(path).await?",
    ] {
        assert!(
            compact_source.contains(expected_call),
            "main entrypoint still bypasses LocalMemoryRuntime: {expected_call}"
        );
    }

    let maintain_start = main_source
        .find("Commands::Maintain {")
        .expect("maintain command arm should exist");
    let maintain_end = main_source[maintain_start..]
        .find("Commands::Welcome {")
        .map(|offset| maintain_start + offset)
        .expect("welcome command arm should follow maintain");
    let maintain_block = &main_source[maintain_start..maintain_end];
    assert!(
        !maintain_block.contains("<SqliteStorage as MaintenanceManager>"),
        "maintain actions still call MaintenanceManager directly"
    );
    assert!(
        !maintain_block.contains("<SqliteStorage as BackupManager>"),
        "maintain backup actions still call BackupManager directly"
    );
}

#[test]
fn maintain_read_actions_preserve_json_and_parameters() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let fixture_path = write_fixture(home.path())?;
    run_cli(home.path(), &["import", &fixture_path])?;

    let (health, _) = compact_json(run_cli(
        home.path(),
        &[
            "maintain",
            "--action",
            "health",
            "--warn-mb",
            "1000",
            "--critical-mb",
            "2000",
            "--max-nodes",
            "100",
        ],
    )?)?;
    assert_eq!(health["status"], "healthy");
    assert_eq!(health["node_count"], 5);
    assert_eq!(health["max_nodes"], 100);
    assert_eq!(health["integrity_ok"], true);
    assert_eq!(health["fts5_indexed"], 5);
    assert_eq!(health["fts5_in_sync"], true);
    assert_eq!(health["warnings"], serde_json::json!([]));

    let (compact, _) = compact_json(run_cli(
        home.path(),
        &[
            "maintain",
            "--action",
            "compact",
            "--event-type",
            "lesson_learned",
            "--similarity-threshold",
            "0.6",
            "--min-cluster-size",
            "3",
            "--dry-run",
        ],
    )?)?;
    assert_eq!(compact["clusters_found"], 1);
    assert_eq!(compact["memories_compacted"], 0);
    assert_eq!(compact["dry_run"], true);
    assert_eq!(compact["clusters"][0]["size"], 3);

    let (fts, _) = compact_json(run_cli(
        home.path(),
        &["maintain", "--action", "fts-rebuild"],
    )?)?;
    assert_eq!(fts["fts_rows_before"], 5);
    assert_eq!(fts["fts_rows_after"], 5);
    assert_eq!(fts["memories_count"], 5);

    Ok(())
}

#[test]
fn maintain_mutations_preserve_counts_and_store_state() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let fixture_path = write_fixture(home.path())?;
    run_cli(home.path(), &["import", &fixture_path])?;

    let (consolidated, _) = compact_json(run_cli(
        home.path(),
        &[
            "maintain",
            "--action",
            "consolidate",
            "--prune-days",
            "30",
            "--max-summaries",
            "50",
        ],
    )?)?;
    assert_eq!(consolidated["before"], 5);
    assert_eq!(consolidated["after"], 4);
    assert_eq!(consolidated["pruned_stale"], 1);
    assert_eq!(consolidated["pruned_summaries"], 0);

    let (cleared, _) = compact_json(run_cli(
        home.path(),
        &[
            "maintain",
            "--action",
            "clear-session",
            "--session-id",
            CLEAR_SESSION,
        ],
    )?)?;
    assert_eq!(
        cleared,
        serde_json::json!({ "session_id": CLEAR_SESSION, "removed": 3 })
    );

    let export: serde_json::Value =
        serde_json::from_slice(&run_cli(home.path(), &["export"])?.stdout)?;
    let memories = export["memories"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("export omitted memories"))?;
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0]["id"], KEEP_ID);
    assert_eq!(export["relationships"], serde_json::json!([]));
    assert_eq!(export["user_profile"]["timezone"], "Asia/Dubai");

    Ok(())
}

#[test]
fn maintain_backup_actions_preserve_restore_contract() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;

    let (baseline, _) = compact_json(run_cli(
        home.path(),
        &[
            "ingest",
            "backup baseline memory",
            "--session-id",
            "backup-session",
        ],
    )?)?;
    let baseline_id = baseline["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("ingest omitted baseline id"))?
        .to_string();

    let (backup, _) = compact_json(run_cli(home.path(), &["maintain", "--action", "backup"])?)?;
    let backup_path = backup["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("backup omitted path"))?
        .to_string();
    assert!(backup["size_bytes"].as_u64().unwrap_or_default() > 0);
    assert!(
        backup["created_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(Path::new(&backup_path).exists());

    let (extra, _) = compact_json(run_cli(
        home.path(),
        &[
            "ingest",
            "post-backup memory",
            "--session-id",
            "backup-session",
        ],
    )?)?;
    let extra_id = extra["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("ingest omitted extra id"))?
        .to_string();

    let (listed, _) = compact_json(run_cli(
        home.path(),
        &["maintain", "--action", "backup-list"],
    )?)?;
    assert!(listed["count"].as_u64().unwrap_or_default() >= 1);
    assert!(
        listed["backups"]
            .as_array()
            .is_some_and(|backups| backups.iter().any(|entry| entry["path"] == backup_path))
    );

    let (restored, _) = compact_json(run_cli(
        home.path(),
        &[
            "maintain",
            "--action",
            "backup-restore",
            "--backup-path",
            &backup_path,
        ],
    )?)?;
    assert_eq!(restored["restored"], true);
    assert_eq!(restored["from"], backup_path);
    assert_eq!(
        restored["note"],
        "restart the server to use the restored database"
    );

    let export: serde_json::Value =
        serde_json::from_slice(&run_cli(home.path(), &["export"])?.stdout)?;
    let ids: Vec<&str> = export["memories"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("export omitted memories"))?
        .iter()
        .filter_map(|memory| memory["id"].as_str())
        .collect();
    assert!(ids.contains(&baseline_id.as_str()));
    assert!(!ids.contains(&extra_id.as_str()));

    Ok(())
}

#[tokio::test]
async fn local_runtime_preserves_maintenance_and_backup_contracts() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let runtime_dir = temp.path().join("runtime");
    let direct_dir = temp.path().join("direct");
    std::fs::create_dir_all(&runtime_dir)?;
    std::fs::create_dir_all(&direct_dir)?;
    let runtime_path = runtime_dir.join("memory.db");
    let direct_path = direct_dir.join("memory.db");
    let runtime_storage = storage_at(runtime_path.clone()).await;
    let direct_storage = storage_at(direct_path.clone()).await;
    let runtime = LocalMemoryRuntime::from_storage(runtime_storage);
    let import_data = serde_json::to_string(&maintenance_fixture())?;

    runtime.import_all(&import_data).await?;
    direct_storage.import_all(&import_data).await?;

    let mut runtime_health = runtime.check_health(1000.0, 2000.0, 100).await?;
    let mut direct_health =
        <SqliteStorage as MaintenanceManager>::check_health(&direct_storage, 1000.0, 2000.0, 100)
            .await?;
    runtime_health
        .as_object_mut()
        .expect("runtime health should be an object")
        .remove("db_size_mb");
    direct_health
        .as_object_mut()
        .expect("direct health should be an object")
        .remove("db_size_mb");
    assert_eq!(runtime_health, direct_health);

    let runtime_compact = runtime.compact("lesson_learned", 0.6, 3, true).await?;
    let direct_compact = <SqliteStorage as MaintenanceManager>::compact(
        &direct_storage,
        "lesson_learned",
        0.6,
        3,
        true,
    )
    .await?;
    assert_eq!(runtime_compact, direct_compact);
    assert_eq!(runtime_compact["clusters_found"], 1);

    let runtime_fts = runtime.rebuild_fts().await?;
    let direct_fts = <SqliteStorage as MaintenanceManager>::rebuild_fts(&direct_storage).await?;
    assert_eq!(runtime_fts, direct_fts);

    let runtime_consolidated = runtime.consolidate(30, 50).await?;
    let direct_consolidated =
        <SqliteStorage as MaintenanceManager>::consolidate(&direct_storage, 30, 50).await?;
    assert_eq!(runtime_consolidated, direct_consolidated);
    assert_eq!(runtime_consolidated["after"], 4);

    let runtime_cleared = runtime.clear_session(CLEAR_SESSION).await?;
    let direct_cleared =
        <SqliteStorage as MaintenanceManager>::clear_session(&direct_storage, CLEAR_SESSION)
            .await?;
    assert_eq!(runtime_cleared, direct_cleared);
    assert_eq!(runtime_cleared, 3);

    let runtime_backup = runtime.create_backup().await?;
    let direct_backup = <SqliteStorage as BackupManager>::create_backup(&direct_storage).await?;
    assert!(runtime_backup.size_bytes > 0);
    assert!(direct_backup.size_bytes > 0);
    assert_eq!(runtime.list_backups().await?.len(), 1);
    assert_eq!(
        <SqliteStorage as BackupManager>::list_backups(&direct_storage)
            .await?
            .len(),
        1
    );
    assert!(runtime.maybe_startup_backup().await?.is_none());
    assert!(
        <SqliteStorage as BackupManager>::maybe_startup_backup(&direct_storage)
            .await?
            .is_none()
    );

    assert!(runtime.delete(KEEP_ID).await?);
    assert!(<SqliteStorage as Deleter>::delete(&direct_storage, KEEP_ID).await?);
    runtime.restore_backup(&runtime_backup.path).await?;
    <SqliteStorage as BackupManager>::restore_backup(&direct_storage, &direct_backup.path).await?;

    let runtime_backups_before_rotation = runtime.list_backups().await?.len();
    let direct_backups_before_rotation =
        <SqliteStorage as BackupManager>::list_backups(&direct_storage)
            .await?
            .len();
    assert_eq!(
        runtime_backups_before_rotation,
        direct_backups_before_rotation
    );
    assert_eq!(
        runtime.rotate_backups(0).await?,
        runtime_backups_before_rotation
    );
    assert_eq!(
        <SqliteStorage as BackupManager>::rotate_backups(&direct_storage, 0).await?,
        direct_backups_before_rotation
    );

    drop(runtime);
    drop(direct_storage);

    let restored_runtime_storage = storage_at(runtime_path).await;
    let restored_direct_storage = storage_at(direct_path).await;
    let runtime_export: serde_json::Value =
        serde_json::from_str(&restored_runtime_storage.export_all().await?)?;
    let direct_export: serde_json::Value =
        serde_json::from_str(&restored_direct_storage.export_all().await?)?;
    assert_eq!(runtime_export, direct_export);
    assert_eq!(runtime_export["memories"].as_array().map(Vec::len), Some(1));
    assert_eq!(runtime_export["memories"][0]["id"], KEEP_ID);
    assert_eq!(runtime_export["user_profile"]["timezone"], "Asia/Dubai");

    Ok(())
}
