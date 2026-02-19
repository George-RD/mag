use std::process::Command;

fn run_cli(home: &std::path::Path, args: &[&str]) -> anyhow::Result<(String, String)> {
    let output = Command::new(env!("CARGO_BIN_EXE_romega-memory"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "command failed: {:?}\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok((
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

#[test]
fn cli_commands_emit_json_payloads() -> anyhow::Result<()> {
    let test_home = std::env::temp_dir().join(format!("romega-cli-smoke-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&test_home)?;

    let (ingest_stdout, _ingest_stderr) = run_cli(&test_home, &["ingest", "hello-cli"])?;
    let ingest_json: serde_json::Value = serde_json::from_str(ingest_stdout.trim())?;
    let id = ingest_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in ingest output"))?
        .to_string();

    let (process_stdout, _process_stderr) = run_cli(&test_home, &["process", "hello-process"])?;
    let process_json: serde_json::Value = serde_json::from_str(process_stdout.trim())?;
    let process_id = process_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in process output"))?
        .to_string();

    let (retrieve_stdout, _retrieve_stderr) = run_cli(&test_home, &["retrieve", &id])?;
    let retrieve_json: serde_json::Value = serde_json::from_str(retrieve_stdout.trim())?;
    assert_eq!(retrieve_json["id"].as_str(), Some(id.as_str()));
    assert_eq!(
        retrieve_json["content"].as_str(),
        Some("processed: hello-cli")
    );

    let (search_stdout, _search_stderr) =
        run_cli(&test_home, &["search", "hello", "--limit", "5"])?;
    let search_json: serde_json::Value = serde_json::from_str(search_stdout.trim())?;
    let results = search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in search output"))?;
    assert!(!results.is_empty());
    assert_eq!(results[0]["id"].as_str(), Some(id.as_str()));

    let (process_retrieve_stdout, _process_retrieve_stderr) =
        run_cli(&test_home, &["retrieve", &process_id])?;
    let process_retrieve_json: serde_json::Value =
        serde_json::from_str(process_retrieve_stdout.trim())?;
    assert_eq!(
        process_retrieve_json["id"].as_str(),
        Some(process_id.as_str())
    );
    assert_eq!(
        process_retrieve_json["content"].as_str(),
        Some("processed: hello-process")
    );

    let (recent_stdout, _recent_stderr) = run_cli(&test_home, &["recent", "--limit", "2"])?;
    let recent_json: serde_json::Value = serde_json::from_str(recent_stdout.trim())?;
    let recent_results = recent_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in recent output"))?;
    assert!(!recent_results.is_empty());
    assert!(
        recent_results
            .iter()
            .any(|entry| entry["id"].as_str() == Some(process_id.as_str()))
    );

    let _ = std::fs::remove_dir_all(&test_home);
    Ok(())
}
