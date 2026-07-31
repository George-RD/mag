use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use mag::LocalMemoryRuntime;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{PlaceholderEmbedder, ProfileManager, WelcomeOptions, WelcomeProvider};

const PROFILE_JSON: &str = r#"{"timezone":"Asia/Dubai"}"#;

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
fn user_context_commands_route_through_local_runtime() {
    let main_source = include_str!("../src/main.rs");
    let compact_source: String = main_source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact_source.contains("local_runtime.get_profile().await?"),
        "profile read still bypasses the selected local runtime"
    );
    assert!(
        compact_source.contains("local_runtime.set_profile(&parsed).await?"),
        "profile update still bypasses the selected local runtime"
    );
    assert!(
        compact_source.contains("local_runtime.welcome_scoped(&opts).await?"),
        "welcome still bypasses the selected local runtime"
    );
    assert!(
        main_source.contains("Profile read through local memory runtime"),
        "profile read does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Profile updated through local memory runtime"),
        "profile update does not report the selected runtime path"
    );
    assert!(
        main_source.contains("Welcome generated through local memory runtime"),
        "welcome does not report the selected runtime path"
    );
}

#[test]
fn profile_and_welcome_commands_preserve_compact_json_contracts() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;

    let update_output = run_cli(home.path(), &["profile", "update", PROFILE_JSON])?;
    let update_stdout = String::from_utf8(update_output.stdout)?;
    let update_stderr = String::from_utf8(update_output.stderr)?;
    let expected_update = serde_json::json!({ "updated": true });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(update_stdout.trim())?,
        expected_update
    );
    assert_eq!(update_stdout, format!("{expected_update}\n"));
    assert!(
        update_stderr.contains("Profile updated through local memory runtime"),
        "profile update did not report the selected runtime path: {update_stderr}"
    );

    let read_output = run_cli(home.path(), &["profile", "read"])?;
    let read_stdout = String::from_utf8(read_output.stdout)?;
    let read_stderr = String::from_utf8(read_output.stderr)?;
    let expected_profile = serde_json::json!({
        "preferences_from_memory": [],
        "timezone": "Asia/Dubai",
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(read_stdout.trim())?,
        expected_profile
    );
    assert_eq!(read_stdout, format!("{expected_profile}\n"));
    assert!(
        read_stderr.contains("Profile read through local memory runtime"),
        "profile read did not report the selected runtime path: {read_stderr}"
    );

    let welcome_output = run_cli(
        home.path(),
        &[
            "welcome",
            "--session-id",
            "context-session",
            "--project",
            "mag",
            "--budget-tokens",
            "200",
            "--agent-type",
            "cli",
            "--entity-id",
            "user:george",
        ],
    )?;
    let welcome_stdout = String::from_utf8(welcome_output.stdout)?;
    let welcome_stderr = String::from_utf8(welcome_output.stderr)?;
    let expected_welcome = serde_json::json!({
        "greeting": "Welcome to MAG! Store your first memory to get started.",
        "memory_count": 0,
        "recent_memories": [],
        "user_context": [],
        "profile": expected_profile,
        "pending_reminders": [],
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(welcome_stdout.trim())?,
        expected_welcome
    );
    assert_eq!(welcome_stdout, format!("{expected_welcome}\n"));
    assert!(
        welcome_stderr.contains("Welcome generated through local memory runtime"),
        "welcome did not report the selected runtime path: {welcome_stderr}"
    );

    Ok(())
}

#[tokio::test]
async fn local_runtime_preserves_profile_and_welcome_contracts() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let storage = storage_at(temp.path().join("memory.db")).await;
    let runtime = LocalMemoryRuntime::from_storage(storage.clone());
    let updates = serde_json::json!({ "timezone": "Asia/Dubai" });

    runtime.set_profile(&updates).await?;
    let runtime_profile = runtime.get_profile().await?;
    let direct_profile = <SqliteStorage as ProfileManager>::get_profile(&storage).await?;
    assert_eq!(runtime_profile, direct_profile);
    assert_eq!(runtime_profile["timezone"], "Asia/Dubai");
    assert_eq!(
        runtime_profile["preferences_from_memory"],
        serde_json::json!([])
    );

    let options = WelcomeOptions {
        session_id: Some("context-session".to_string()),
        project: Some("mag".to_string()),
        budget_tokens: Some(200),
        agent_type: Some("cli".to_string()),
        entity_id: Some("user:george".to_string()),
    };
    let runtime_welcome = runtime.welcome_scoped(&options).await?;
    let direct_welcome =
        <SqliteStorage as WelcomeProvider>::welcome_scoped(&storage, &options).await?;
    assert_eq!(runtime_welcome, direct_welcome);
    assert_eq!(runtime_welcome["memory_count"], 0);
    assert_eq!(runtime_welcome["profile"], runtime_profile);
    assert_eq!(runtime_welcome["recent_memories"], serde_json::json!([]));
    assert_eq!(runtime_welcome["user_context"], serde_json::json!([]));
    assert_eq!(runtime_welcome["pending_reminders"], serde_json::json!([]));

    Ok(())
}
