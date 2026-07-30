use std::path::Path;
use std::process::{Command, Output};

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

fn assert_store_command_contract(
    home: &Path,
    command: &str,
    content: &str,
) -> anyhow::Result<()> {
    let output = run_cli(
        home,
        &[
            command,
            content,
            "--tags",
            "cli,runtime",
            "--importance",
            "0.75",
            "--metadata",
            r#"{"source":"cli-parity"}"#,
            "--event-type",
            "decision",
            "--session-id",
            "session-1",
            "--project",
            "mag",
            "--priority",
            "8",
            "--entity-id",
            "runtime-facade",
            "--agent-type",
            "cli",
            "--ttl-seconds",
            "3600",
            "--referenced-date",
            "2026-07-30T00:00:00Z",
        ],
    )?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
    let id = payload["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in {command} output"))?;

    assert_eq!(stdout, format!("{}\n", serde_json::json!({ "id": id })));
    assert!(
        stderr.contains("Stored through local memory runtime"),
        "{command} did not report the selected runtime path: {stderr}"
    );

    let retrieve = run_cli(home, &["retrieve", id])?;
    let retrieve_payload: serde_json::Value =
        serde_json::from_slice(retrieve.stdout.as_slice())?;
    assert_eq!(retrieve_payload["id"].as_str(), Some(id));
    assert_eq!(
        retrieve_payload["content"].as_str(),
        Some(format!("processed: {content}").as_str())
    );

    Ok(())
}

#[test]
fn ingest_and_process_use_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!(
        "mag-cli-store-runtime-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home)?;

    assert_store_command_contract(&home, "ingest", "ingest caller parity")?;
    assert_store_command_contract(&home, "process", "process caller parity")?;

    std::fs::remove_dir_all(home)?;
    Ok(())
}
