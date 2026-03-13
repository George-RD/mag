use chrono::{Duration, Utc};
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

fn assert_entity_agent_fields(
    result: &serde_json::Value,
    entity_id: &str,
    agent_type: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        result["entity_id"].as_str() == Some(entity_id),
        "missing or incorrect entity_id in result payload: {result}"
    );
    anyhow::ensure!(
        result["agent_type"].as_str() == Some(agent_type),
        "missing or incorrect agent_type in result payload: {result}"
    );
    Ok(())
}

#[test]
fn cli_commands_emit_json_payloads() -> anyhow::Result<()> {
    let test_home = std::env::temp_dir().join(format!("romega-cli-smoke-{}", uuid::Uuid::new_v4()));
    let created_cutoff = (Utc::now() - Duration::days(1)).to_rfc3339();
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

    let (semantic_stdout, _semantic_stderr) =
        run_cli(&test_home, &["semantic-search", "hello", "--limit", "5"])?;
    let semantic_json: serde_json::Value = serde_json::from_str(semantic_stdout.trim())?;
    let semantic_results = semantic_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in semantic output"))?;
    assert!(!semantic_results.is_empty());
    assert!(semantic_results[0]["score"].as_f64().is_some());

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

    let (delete_stdout, _delete_stderr) = run_cli(&test_home, &["delete", &id])?;
    let delete_json: serde_json::Value = serde_json::from_str(delete_stdout.trim())?;
    assert_eq!(delete_json["deleted"].as_bool(), Some(true));

    let (reingest_stdout, _) = run_cli(&test_home, &["ingest", "test-data", "--tags", "a,b"])?;
    let reingest_json: serde_json::Value = serde_json::from_str(reingest_stdout.trim())?;
    let reingest_id = reingest_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id"))?
        .to_string();

    let (update_stdout, _update_stderr) = run_cli(
        &test_home,
        &["update", &reingest_id, "--content", "updated-data"],
    )?;
    let update_json: serde_json::Value = serde_json::from_str(update_stdout.trim())?;
    assert_eq!(update_json["updated"].as_bool(), Some(true));

    let (list_stdout, _list_stderr) = run_cli(&test_home, &["list", "--limit", "10"])?;
    let list_json: serde_json::Value = serde_json::from_str(list_stdout.trim())?;
    assert!(list_json["total"].as_u64().is_some());
    assert!(list_json["results"].as_array().is_some());

    let (relations_stdout, _relations_stderr) = run_cli(&test_home, &["relations", &reingest_id])?;
    let relations_json: serde_json::Value = serde_json::from_str(relations_stdout.trim())?;
    assert!(relations_json["relationships"].as_array().is_some());

    let (stats_stdout, _stats_stderr) = run_cli(&test_home, &["stats"])?;
    let stats_json: serde_json::Value = serde_json::from_str(stats_stdout.trim())?;
    assert!(stats_json["total_memories"].as_u64().is_some());
    assert!(stats_json["fts5_indexed"].as_u64().is_some());

    let (export_stdout, _export_stderr) = run_cli(&test_home, &["export"])?;
    let export_json: serde_json::Value = serde_json::from_str(export_stdout.trim())?;
    assert!(export_json["memories"].as_array().is_some());
    assert!(export_json["version"].as_u64() == Some(1));

    // Write exported data to a temp file, then import it
    let import_file = test_home.join("export.json");
    std::fs::write(&import_file, &export_stdout)?;

    // Delete existing data first
    run_cli(&test_home, &["delete", &reingest_id])?;
    run_cli(&test_home, &["delete", &process_id])?;

    let (import_stdout, _import_stderr) =
        run_cli(&test_home, &["import", import_file.to_str().unwrap()])?;
    let import_json: serde_json::Value = serde_json::from_str(import_stdout.trim())?;
    assert!(import_json["imported_memories"].as_u64().unwrap() > 0);

    let (ingest_imp_stdout, _) = run_cli(
        &test_home,
        &[
            "ingest",
            "important-data",
            "--importance",
            "0.9",
            "--metadata",
            r#"{"key":"val"}"#,
        ],
    )?;
    let ingest_imp_json: serde_json::Value = serde_json::from_str(ingest_imp_stdout.trim())?;
    assert!(ingest_imp_json["id"].as_str().is_some());

    // Verify search results include importance field
    let (search_stdout, _search_stderr) =
        run_cli(&test_home, &["search", "important", "--limit", "5"])?;
    let search_json: serde_json::Value = serde_json::from_str(search_stdout.trim())?;
    let search_results = search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in search output"))?;
    if !search_results.is_empty() {
        assert!(search_results[0]["importance"].as_f64().is_some());
    }

    run_cli(
        &test_home,
        &[
            "ingest",
            "filter-target low importance",
            "--importance",
            "0.2",
            "--referenced-date",
            "2026-01-01T00:00:00Z",
        ],
    )?;
    let (high_filter_stdout, _) = run_cli(
        &test_home,
        &[
            "ingest",
            "filter-target high importance",
            "--importance",
            "0.95",
            "--tags",
            "focus",
            "--referenced-date",
            "2026-03-01T00:00:00Z",
        ],
    )?;
    let high_filter_json: serde_json::Value = serde_json::from_str(high_filter_stdout.trim())?;
    let high_filter_id = high_filter_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in filtered ingest output"))?
        .to_string();
    let (control_filter_stdout, _) = run_cli(
        &test_home,
        &[
            "ingest",
            "filter-target high importance control",
            "--importance",
            "0.95",
            "--tags",
            "other",
            "--referenced-date",
            "2026-03-01T00:00:00Z",
        ],
    )?;
    let control_filter_json: serde_json::Value =
        serde_json::from_str(control_filter_stdout.trim())?;
    let control_filter_id = control_filter_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in control ingest output"))?
        .to_string();

    let (filtered_search_stdout, _) = run_cli(
        &test_home,
        &[
            "search",
            "filter-target",
            "--importance-min",
            "0.9",
            "--context-tags",
            "focus",
            "--event-after",
            "2026-02-01T00:00:00Z",
        ],
    )?;
    let filtered_search_json: serde_json::Value =
        serde_json::from_str(filtered_search_stdout.trim())?;
    let filtered_results = filtered_search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in filtered search output"))?;
    assert_eq!(filtered_results.len(), 1);
    assert_eq!(
        filtered_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !filtered_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );
    assert!(
        filtered_results[0]["importance"]
            .as_f64()
            .is_some_and(|importance| importance >= 0.9)
    );

    let (semantic_search_stdout, _) = run_cli(
        &test_home,
        &[
            "semantic-search",
            "filter-target",
            "--importance-min",
            "0.9",
            "--context-tags",
            "focus",
            "--event-after",
            "2026-02-01T00:00:00Z",
        ],
    )?;
    let semantic_search_json: serde_json::Value =
        serde_json::from_str(semantic_search_stdout.trim())?;
    let semantic_results = semantic_search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in semantic search output"))?;
    assert_eq!(semantic_results.len(), 1);
    assert_eq!(
        semantic_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !semantic_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );

    let (advanced_search_stdout, _) = run_cli(
        &test_home,
        &[
            "advanced-search",
            "filter-target",
            "--importance-min",
            "0.9",
            "--context-tags",
            "focus",
            "--event-after",
            "2026-02-01T00:00:00Z",
        ],
    )?;
    let advanced_search_json: serde_json::Value =
        serde_json::from_str(advanced_search_stdout.trim())?;
    let advanced_results = advanced_search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in advanced search output"))?;
    assert_eq!(advanced_results.len(), 1);
    assert_eq!(
        advanced_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !advanced_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );

    let (phrase_search_stdout, _) = run_cli(
        &test_home,
        &[
            "phrase-search",
            "filter-target",
            "--context-tags",
            "focus",
            "--event-after",
            "2026-02-01T00:00:00Z",
        ],
    )?;
    let phrase_search_json: serde_json::Value = serde_json::from_str(phrase_search_stdout.trim())?;
    let phrase_results = phrase_search_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in phrase search output"))?;
    assert_eq!(phrase_results.len(), 1);
    assert_eq!(
        phrase_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !phrase_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );

    let (filtered_list_stdout, _) = run_cli(
        &test_home,
        &[
            "list",
            "--limit",
            "5",
            "--importance-min",
            "0.9",
            "--created-after",
            &created_cutoff,
            "--context-tags",
            "focus",
        ],
    )?;
    let filtered_list_json: serde_json::Value = serde_json::from_str(filtered_list_stdout.trim())?;
    let filtered_list_results = filtered_list_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in filtered list output"))?;
    assert_eq!(filtered_list_results.len(), 1);
    assert_eq!(
        filtered_list_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !filtered_list_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );

    let (filtered_recent_stdout, _) = run_cli(
        &test_home,
        &[
            "recent",
            "--limit",
            "5",
            "--importance-min",
            "0.9",
            "--created-after",
            &created_cutoff,
            "--context-tags",
            "focus",
        ],
    )?;
    let filtered_recent_json: serde_json::Value =
        serde_json::from_str(filtered_recent_stdout.trim())?;
    let filtered_recent_results = filtered_recent_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in filtered recent output"))?;
    assert_eq!(filtered_recent_results.len(), 1);
    assert_eq!(
        filtered_recent_results[0]["id"].as_str(),
        Some(high_filter_id.as_str())
    );
    assert!(
        !filtered_recent_results
            .iter()
            .any(|result| result["id"].as_str() == Some(control_filter_id.as_str()))
    );

    let (entity_target_stdout, _) = run_cli(
        &test_home,
        &[
            "ingest",
            "entity-agent target",
            "--entity-id",
            "issue-123",
            "--agent-type",
            "planner",
            "--referenced-date",
            "2026-03-02T00:00:00Z",
        ],
    )?;
    let entity_target_json: serde_json::Value = serde_json::from_str(entity_target_stdout.trim())?;
    let entity_target_id = entity_target_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in entity target output"))?
        .to_string();

    let (entity_control_stdout, _) = run_cli(
        &test_home,
        &[
            "ingest",
            "entity-agent target",
            "--entity-id",
            "issue-999",
            "--agent-type",
            "executor",
            "--referenced-date",
            "2026-03-02T00:00:00Z",
        ],
    )?;
    let entity_control_json: serde_json::Value =
        serde_json::from_str(entity_control_stdout.trim())?;
    let entity_control_id = entity_control_json["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing id in entity control output"))?
        .to_string();

    let (entity_advanced_stdout, _) = run_cli(
        &test_home,
        &[
            "advanced-search",
            "entity-agent target",
            "--entity-id",
            "issue-123",
            "--agent-type",
            "planner",
            "--explain",
        ],
    )?;
    let entity_advanced_json: serde_json::Value =
        serde_json::from_str(entity_advanced_stdout.trim())?;
    let entity_advanced_results = entity_advanced_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in entity advanced search output"))?;
    assert_eq!(entity_advanced_results.len(), 1);
    assert_eq!(
        entity_advanced_results[0]["id"].as_str(),
        Some(entity_target_id.as_str())
    );
    assert_ne!(
        entity_advanced_results[0]["id"].as_str(),
        Some(entity_control_id.as_str())
    );
    assert_entity_agent_fields(&entity_advanced_results[0], "issue-123", "planner")?;
    assert!(entity_advanced_results[0]["metadata"]["_explain"].is_object());

    let (entity_list_stdout, _) = run_cli(
        &test_home,
        &[
            "list",
            "--limit",
            "5",
            "--entity-id",
            "issue-123",
            "--agent-type",
            "planner",
        ],
    )?;
    let entity_list_json: serde_json::Value = serde_json::from_str(entity_list_stdout.trim())?;
    let entity_list_results = entity_list_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in entity list output"))?;
    assert_eq!(entity_list_results.len(), 1);
    assert_eq!(
        entity_list_results[0]["id"].as_str(),
        Some(entity_target_id.as_str())
    );
    assert_entity_agent_fields(&entity_list_results[0], "issue-123", "planner")?;

    let (entity_recent_stdout, _) = run_cli(
        &test_home,
        &[
            "recent",
            "--limit",
            "5",
            "--entity-id",
            "issue-123",
            "--agent-type",
            "planner",
        ],
    )?;
    let entity_recent_json: serde_json::Value = serde_json::from_str(entity_recent_stdout.trim())?;
    let entity_recent_results = entity_recent_json["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in entity recent output"))?;
    assert_eq!(entity_recent_results.len(), 1);
    assert_eq!(
        entity_recent_results[0]["id"].as_str(),
        Some(entity_target_id.as_str())
    );
    assert_ne!(
        entity_recent_results[0]["id"].as_str(),
        Some(entity_control_id.as_str())
    );
    assert_entity_agent_fields(&entity_recent_results[0], "issue-123", "planner")?;

    let _ = std::fs::remove_dir_all(&test_home);
    Ok(())
}
