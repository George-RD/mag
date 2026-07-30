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

fn assert_retrieved_content(home: &Path, id: &str, expected: &str) -> anyhow::Result<()> {
    let retrieve = run_cli(home, &["retrieve", id])?;
    let retrieve_payload: serde_json::Value = serde_json::from_slice(retrieve.stdout.as_slice())?;
    let retrieve_stderr = String::from_utf8(retrieve.stderr)?;
    assert_eq!(retrieve_payload["id"].as_str(), Some(id));
    assert_eq!(retrieve_payload["content"].as_str(), Some(expected));
    assert!(
        retrieve_stderr.contains("Retrieved through local memory runtime"),
        "retrieve did not report the selected runtime path: {retrieve_stderr}"
    );
    Ok(())
}

fn assert_list_and_relationship_contracts(
    home: &Path,
    id: &str,
    expected_content: &str,
) -> anyhow::Result<()> {
    let list = run_cli(
        home,
        &[
            "list",
            "--offset",
            "0",
            "--limit",
            "1",
            "--event-type",
            "lesson_learned",
            "--project",
            "mag",
            "--session-id",
            "session-1",
        ],
    )?;
    let list_stdout = String::from_utf8(list.stdout)?;
    let list_stderr = String::from_utf8(list.stderr)?;
    let expected_list = serde_json::json!({
        "results": [{
            "id": id,
            "content": expected_content,
            "tags": ["updated", "runtime"],
            "importance": 0.9,
            "metadata": {"source": "cli-update-parity"},
            "event_type": "lesson_learned",
            "session_id": "session-1",
            "project": "mag",
            "entity_id": "runtime-facade",
            "agent_type": "cli"
        }],
        "total": 1
    });
    assert_eq!(list_stdout, format!("{expected_list}\n"));
    assert!(
        list_stderr.contains("Listed through local memory runtime"),
        "list did not report the selected runtime path: {list_stderr}"
    );

    let relations = run_cli(home, &["relations", id])?;
    let relations_stdout = String::from_utf8(relations.stdout)?;
    let relations_stderr = String::from_utf8(relations.stderr)?;
    assert_eq!(
        relations_stdout,
        format!("{}\n", serde_json::json!({ "relationships": [] }))
    );
    assert!(
        relations_stderr.contains("Relationships retrieved through local memory runtime"),
        "relations did not report the selected runtime path: {relations_stderr}"
    );

    Ok(())
}

fn assert_store_command_contract(home: &Path, command: &str, content: &str) -> anyhow::Result<()> {
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

    let expected_content = format!("processed: {content}");
    assert_retrieved_content(home, id, expected_content.as_str())?;

    let updated_content = format!("{command} updated caller parity");
    let update = run_cli(
        home,
        &[
            "update",
            id,
            "--content",
            updated_content.as_str(),
            "--tags",
            "updated,runtime",
            "--importance",
            "0.9",
            "--metadata",
            r#"{"source":"cli-update-parity"}"#,
            "--event-type",
            "lesson_learned",
            "--priority",
            "9",
        ],
    )?;
    let update_stdout = String::from_utf8(update.stdout)?;
    let update_stderr = String::from_utf8(update.stderr)?;
    assert_eq!(
        update_stdout,
        format!("{}\n", serde_json::json!({ "id": id, "updated": true }))
    );
    assert!(
        update_stderr.contains("Updated through local memory runtime"),
        "update did not report the selected runtime path: {update_stderr}"
    );

    assert_retrieved_content(home, id, updated_content.as_str())?;
    assert_list_and_relationship_contracts(home, id, updated_content.as_str())?;

    let delete = run_cli(home, &["delete", id])?;
    let delete_stdout = String::from_utf8(delete.stdout)?;
    let delete_stderr = String::from_utf8(delete.stderr)?;
    let delete_payload: serde_json::Value = serde_json::from_str(delete_stdout.trim())?;
    assert_eq!(
        delete_stdout,
        format!("{}\n", serde_json::json!({ "id": id, "deleted": true }))
    );
    assert_eq!(delete_payload["id"].as_str(), Some(id));
    assert_eq!(delete_payload["deleted"].as_bool(), Some(true));
    assert!(
        delete_stderr.contains("Deleted through local memory runtime"),
        "delete did not report the selected runtime path: {delete_stderr}"
    );

    Ok(())
}

#[test]
fn cli_crud_commands_use_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("mag-cli-store-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home)?;

    assert_store_command_contract(&home, "ingest", "ingest caller parity")?;
    assert_store_command_contract(&home, "process", "process caller parity")?;

    std::fs::remove_dir_all(home)?;
    Ok(())
}
