use std::path::Path;
use std::process::{Command, Output};

const TEXT: &str = "review the reminder runtime migration";
const DURATION: &str = "2h";
const CONTEXT: &str = "after the checkpoint slice";
const SESSION_ID: &str = "reminder-runtime-session";
const PROJECT: &str = "runtime-migration";

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

fn required_string(value: &serde_json::Value, field: &str) -> anyhow::Result<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("reminder payload omitted {field}"))
}

fn assert_rfc3339(value: &str) -> anyhow::Result<()> {
    chrono::DateTime::parse_from_rfc3339(value)?;
    Ok(())
}

#[test]
fn reminder_commands_use_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;

    let created = run_cli(
        home.path(),
        &[
            "remind",
            "set",
            "--text",
            TEXT,
            "--duration",
            DURATION,
            "--context",
            CONTEXT,
            "--session-id",
            SESSION_ID,
            "--project",
            PROJECT,
        ],
    )?;
    let created_stdout = String::from_utf8(created.stdout)?;
    let created_stderr = String::from_utf8(created.stderr)?;
    let created_payload: serde_json::Value = serde_json::from_str(created_stdout.trim())?;
    let reminder_id = required_string(&created_payload, "reminder_id")?;
    let remind_at = required_string(&created_payload, "remind_at")?;
    uuid::Uuid::parse_str(&reminder_id)?;
    assert_rfc3339(&remind_at)?;
    let expected_created = serde_json::json!({
        "reminder_id": reminder_id,
        "text": TEXT,
        "remind_at": remind_at,
        "duration": DURATION,
    });
    assert_eq!(created_stdout, format!("{expected_created}\n"));
    assert!(
        created_stderr.contains("Reminder created through local memory runtime"),
        "remind set did not report the selected runtime path: {created_stderr}"
    );

    let pending = run_cli(home.path(), &["remind", "list", "--status", "pending"])?;
    let pending_stdout = String::from_utf8(pending.stdout)?;
    let pending_stderr = String::from_utf8(pending.stderr)?;
    let pending_payload: serde_json::Value = serde_json::from_str(pending_stdout.trim())?;
    let pending_results = pending_payload["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("remind list omitted results"))?;
    anyhow::ensure!(pending_results.len() == 1, "expected one pending reminder");
    let pending_entry = &pending_results[0];
    let created_at = required_string(pending_entry, "created_at")?;
    let created_at_utc = required_string(&pending_entry["metadata"], "created_at_utc")?;
    assert_rfc3339(&created_at)?;
    assert_rfc3339(&created_at_utc)?;
    let expected_pending = serde_json::json!({
        "results": [{
            "reminder_id": reminder_id,
            "text": format!("{TEXT}\n[due: {remind_at}]"),
            "status": "pending",
            "remind_at": remind_at,
            "is_due": false,
            "is_overdue": false,
            "metadata": {
                "event_type": "reminder",
                "reminder_status": "pending",
                "remind_at": remind_at,
                "created_at_utc": created_at_utc,
                "context": CONTEXT,
                "session_id": SESSION_ID,
                "project": PROJECT,
            },
            "created_at": created_at,
        }]
    });
    assert_eq!(pending_payload, expected_pending);
    assert_eq!(pending_stdout, format!("{expected_pending}\n"));
    assert!(
        pending_stderr.contains("Reminders listed through local memory runtime"),
        "remind list did not report the selected runtime path: {pending_stderr}"
    );

    let dismissed = run_cli(
        home.path(),
        &["remind", "dismiss", "--reminder-id", &reminder_id],
    )?;
    let dismissed_stdout = String::from_utf8(dismissed.stdout)?;
    let dismissed_stderr = String::from_utf8(dismissed.stderr)?;
    let dismissed_payload: serde_json::Value = serde_json::from_str(dismissed_stdout.trim())?;
    let dismissed_at = required_string(&dismissed_payload, "dismissed_at")?;
    assert_rfc3339(&dismissed_at)?;
    let expected_dismissed = serde_json::json!({
        "reminder_id": reminder_id,
        "status": "dismissed",
        "dismissed_at": dismissed_at,
    });
    assert_eq!(dismissed_stdout, format!("{expected_dismissed}\n"));
    assert!(
        dismissed_stderr.contains("Reminder dismissed through local memory runtime"),
        "remind dismiss did not report the selected runtime path: {dismissed_stderr}"
    );

    let no_pending = run_cli(home.path(), &["remind", "list", "--status", "pending"])?;
    assert_eq!(String::from_utf8(no_pending.stdout)?, "{\"results\":[]}\n");

    let dismissed_list = run_cli(home.path(), &["remind", "list", "--status", "dismissed"])?;
    let dismissed_list_stdout = String::from_utf8(dismissed_list.stdout)?;
    let dismissed_list_payload: serde_json::Value =
        serde_json::from_str(dismissed_list_stdout.trim())?;
    let expected_dismissed_list = serde_json::json!({
        "results": [{
            "reminder_id": reminder_id,
            "text": format!("{TEXT}\n[due: {remind_at}]"),
            "status": "dismissed",
            "remind_at": remind_at,
            "is_due": false,
            "is_overdue": false,
            "metadata": {
                "event_type": "reminder",
                "reminder_status": "dismissed",
                "remind_at": remind_at,
                "created_at_utc": created_at_utc,
                "context": CONTEXT,
                "session_id": SESSION_ID,
                "project": PROJECT,
                "dismissed_at": dismissed_at,
            },
            "created_at": created_at,
        }]
    });
    assert_eq!(dismissed_list_payload, expected_dismissed_list);
    assert_eq!(
        dismissed_list_stdout,
        format!("{expected_dismissed_list}\n")
    );

    Ok(())
}
