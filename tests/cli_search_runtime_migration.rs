use std::path::Path;
use std::process::{Command, Output};

const CONTENT: &str = "searchruntimeanchor durable local context";
const QUERY: &str = "searchruntimeanchor";

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

fn seed_memory(home: &Path) -> anyhow::Result<String> {
    let output = run_cli(
        home,
        &[
            "ingest",
            CONTENT,
            "--tags",
            "runtime,search",
            "--importance",
            "0.8",
            "--metadata",
            r#"{"source":"cli-search-parity"}"#,
            "--event-type",
            "decision",
            "--session-id",
            "search-session",
            "--project",
            "mag",
            "--entity-id",
            "runtime-facade",
            "--agent-type",
            "cli",
            "--referenced-date",
            "2026-07-30T00:00:00Z",
        ],
    )?;
    let stdout = String::from_utf8(output.stdout)?;
    let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
    let id = payload["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in ingest output"))?
        .to_string();
    assert_eq!(stdout, format!("{}\n", serde_json::json!({ "id": id })));
    Ok(id)
}

fn search_args(command: &str) -> Vec<&str> {
    vec![
        command,
        QUERY,
        "--limit",
        "1",
        "--event-type",
        "decision",
        "--project",
        "mag",
        "--session-id",
        "search-session",
        "--entity-id",
        "runtime-facade",
        "--agent-type",
        "cli",
        "--importance-min",
        "0.7",
        "--context-tags",
        "runtime",
    ]
}

fn expected_result(id: &str, score: Option<f64>) -> serde_json::Value {
    let mut result = serde_json::json!({
        "id": id,
        "content": format!("processed: {CONTENT}"),
        "tags": ["runtime", "search"],
        "importance": 0.8,
        "metadata": {"source": "cli-search-parity"},
        "event_type": "decision",
        "session_id": "search-session",
        "project": "mag",
        "entity_id": "runtime-facade",
        "agent_type": "cli"
    });
    if let Some(score) = score {
        result["score"] = serde_json::json!(score);
    }
    result
}

fn assert_unscored_search_contract(
    command: &str,
    expected_runtime_log: &str,
) -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!(
        "mag-cli-{command}-runtime-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home)?;
    let id = seed_memory(&home)?;

    let output = run_cli(&home, search_args(command).as_slice())?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let expected = serde_json::json!({ "results": [expected_result(&id, None)] });
    assert_eq!(stdout, format!("{expected}\n"));
    assert!(
        stderr.contains(expected_runtime_log),
        "{command} did not report the selected runtime path: {stderr}"
    );

    std::fs::remove_dir_all(home)?;
    Ok(())
}

fn assert_scored_search_contract(command: &str, expected_runtime_log: &str) -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!(
        "mag-cli-{command}-runtime-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home)?;
    let id = seed_memory(&home)?;

    let output = run_cli(&home, search_args(command).as_slice())?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
    let results = payload["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in {command} output"))?;
    anyhow::ensure!(
        results.len() == 1,
        "unexpected {command} results: {payload}"
    );
    let score = results[0]["score"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("missing score in {command} output"))?;
    let expected = serde_json::json!({ "results": [expected_result(&id, Some(score))] });
    assert_eq!(stdout, format!("{expected}\n"));
    assert!(
        stderr.contains(expected_runtime_log),
        "{command} did not report the selected runtime path: {stderr}"
    );

    std::fs::remove_dir_all(home)?;
    Ok(())
}

#[test]
fn search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_unscored_search_contract("search", "Search completed through local memory runtime")
}

#[test]
fn semantic_search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_scored_search_contract(
        "semantic-search",
        "Semantic search completed through local memory runtime",
    )
}

#[test]
fn advanced_search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_scored_search_contract(
        "advanced-search",
        "Advanced search completed through local memory runtime",
    )
}
