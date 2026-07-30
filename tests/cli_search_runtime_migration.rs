use std::path::Path;
use std::process::{Command, Output};

const CONTENT: &str = "searchruntimeanchor durable local context";
const QUERY: &str = "searchruntimeanchor";

#[derive(Clone, Copy)]
struct RetrievalContract {
    command: &'static str,
    query: Option<&'static str>,
    runtime_log: &'static str,
    scored: bool,
    expects_text_overlap: bool,
}

const BASIC_SEARCH: RetrievalContract = RetrievalContract {
    command: "search",
    query: Some(QUERY),
    runtime_log: "Search completed through local memory runtime",
    scored: false,
    expects_text_overlap: false,
};
const SEMANTIC_SEARCH: RetrievalContract = RetrievalContract {
    command: "semantic-search",
    query: Some(QUERY),
    runtime_log: "Semantic search completed through local memory runtime",
    scored: true,
    expects_text_overlap: false,
};
const ADVANCED_SEARCH: RetrievalContract = RetrievalContract {
    command: "advanced-search",
    query: Some(QUERY),
    runtime_log: "Advanced search completed through local memory runtime",
    scored: true,
    expects_text_overlap: true,
};
const RECENT: RetrievalContract = RetrievalContract {
    command: "recent",
    query: None,
    runtime_log: "Recent list completed through local memory runtime",
    scored: false,
    expects_text_overlap: false,
};

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

fn retrieval_args(contract: RetrievalContract) -> Vec<&'static str> {
    let mut args = vec![contract.command];
    if let Some(query) = contract.query {
        args.push(query);
    }
    args.extend([
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
    ]);
    args
}

fn expected_result(id: &str, score: Option<f64>, include_text_overlap: bool) -> serde_json::Value {
    let metadata = if include_text_overlap {
        serde_json::json!({
            "_text_overlap": 1.0,
            "source": "cli-search-parity"
        })
    } else {
        serde_json::json!({"source": "cli-search-parity"})
    };
    let mut result = serde_json::json!({
        "id": id,
        "content": format!("processed: {CONTENT}"),
        "tags": ["runtime", "search"],
        "importance": 0.8,
        "metadata": metadata,
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

fn assert_retrieval_contract(contract: RetrievalContract) -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let id = seed_memory(home.path())?;

    let output = run_cli(home.path(), retrieval_args(contract).as_slice())?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
    let results = payload["results"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing results in {} output", contract.command))?;
    anyhow::ensure!(
        results.len() == 1,
        "unexpected {} results: {payload}",
        contract.command
    );
    let score = if contract.scored {
        Some(
            results[0]["score"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("missing score in {} output", contract.command))?,
        )
    } else {
        anyhow::ensure!(
            results[0].get("score").is_none(),
            "unexpected score in {} output",
            contract.command
        );
        None
    };
    let expected = serde_json::json!({
        "results": [expected_result(&id, score, contract.expects_text_overlap)]
    });
    assert_eq!(stdout, format!("{expected}\n"));
    assert!(
        stderr.contains(contract.runtime_log),
        "{} did not report the selected runtime path: {stderr}",
        contract.command
    );

    Ok(())
}

#[test]
fn search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_retrieval_contract(BASIC_SEARCH)
}

#[test]
fn semantic_search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_retrieval_contract(SEMANTIC_SEARCH)
}

#[test]
fn advanced_search_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_retrieval_contract(ADVANCED_SEARCH)
}

#[test]
fn recent_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    assert_retrieval_contract(RECENT)
}
