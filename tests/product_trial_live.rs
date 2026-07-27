//! Temporary black-box product trial. Do not merge.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn mag() -> &'static str {
    env!("CARGO_BIN_EXE_mag")
}

fn run(home: &TempDir, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(mag())
        .args(args)
        .env("MAG_DATA_ROOT", home.path().join(".mag"))
        .env("HOME", home.path())
        .output()
        .expect("spawn mag");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    eprintln!(
        "\n$ mag {}\nexit={code}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        args.join(" ")
    );
    (code, stdout, stderr)
}

fn success(home: &TempDir, args: &[&str]) -> String {
    let (code, stdout, stderr) = run(home, args);
    assert_eq!(code, 0, "command failed: {args:?}\n{stderr}");
    stdout
}

#[test]
fn black_box_product_trial() {
    let home = TempDir::new().expect("temp home");

    eprintln!("=== discoverability / clean state ===");
    success(&home, &["--version"]);
    success(&home, &["--help"]);
    let _ = run(&home, &["doctor"]);

    eprintln!("=== basic and semantic recall ===");
    success(
        &home,
        &[
            "ingest",
            "The deployment region for Project Atlas is eu-central-1.",
        ],
    );
    let result = success(&home, &["search", "Where is Atlas deployed?"]);
    assert!(result.contains("eu-central-1"), "exact fact missed: {result}");

    success(
        &home,
        &[
            "ingest",
            "Retries must use exponential backoff with random jitter and stop after five attempts.",
        ],
    );
    let result = success(
        &home,
        &["search", "What is our policy when requests keep failing?"],
    );
    assert!(
        result.to_lowercase().contains("backoff") || result.to_lowercase().contains("jitter"),
        "paraphrase recall failed: {result}"
    );

    eprintln!("=== persistence and duplicate behavior ===");
    let result = success(&home, &["search", "Atlas deployment region"]);
    assert!(result.contains("eu-central-1"));
    success(
        &home,
        &[
            "ingest",
            "The deployment region for Project Atlas is eu-central-1.",
        ],
    );
    eprintln!("duplicate result: {}", success(&home, &["search", "Atlas region"]));

    eprintln!("=== conflicting / updated memory ===");
    success(
        &home,
        &[
            "ingest",
            "Project Atlas moved from eu-central-1 to me-central-1. The current deployment region is me-central-1.",
        ],
    );
    let result = success(
        &home,
        &["search", "What is the current deployment region for Project Atlas?"],
    );
    eprintln!("conflict result: {result}");
    assert!(result.contains("me-central-1"), "new fact not surfaced: {result}");

    eprintln!("=== negation and distractor resistance ===");
    success(
        &home,
        &["ingest", "Project Borealis uses PostgreSQL, not MySQL."],
    );
    success(
        &home,
        &[
            "ingest",
            "A discarded prototype once used MySQL for an unrelated demo.",
        ],
    );
    let result = success(&home, &["search", "Which database does Borealis use?"]);
    assert!(
        result.to_lowercase().contains("postgres"),
        "distractor displaced fact: {result}"
    );

    eprintln!("=== unicode and multiline ===");
    success(
        &home,
        &[
            "ingest",
            "Client note: café access code is ٤٢.\nOwner: Zoë.\nDo not normalize identifier A/B-17.",
        ],
    );
    let result = success(
        &home,
        &[
            "search",
            "Who owns the cafe access note and which identifier must remain unchanged?",
        ],
    );
    assert!(
        result.contains("Zoë") || result.contains("A/B-17"),
        "unicode recall failed: {result}"
    );

    eprintln!("=== empty and whitespace inputs ===");
    let empty_ingest = run(&home, &["ingest", ""]);
    let empty_search = run(&home, &["search", ""]);
    let whitespace_search = run(&home, &["search", "   "]);
    eprintln!(
        "empty exits: ingest={} search={} whitespace={}",
        empty_ingest.0, empty_search.0, whitespace_search.0
    );

    eprintln!("=== oversized input ===");
    let large = format!(
        "Large-context sentinel MAG-LARGE-9472. {}",
        "alpha beta gamma delta ".repeat(20_000)
    );
    let started = Instant::now();
    let large_result = run(&home, &["ingest", &large]);
    assert!(started.elapsed() < Duration::from_secs(60));
    eprintln!("large input exit={}", large_result.0);
    if large_result.0 == 0 {
        let result = success(&home, &["search", "What is the large-context sentinel?"]);
        assert!(result.contains("MAG-LARGE-9472"));
    }

    eprintln!("=== rapid sequential writes ===");
    let started = Instant::now();
    for i in 1..=100 {
        let text = format!("Load-test memory item {i}, token LOAD-{i}");
        success(&home, &["ingest", &text]);
    }
    eprintln!("100 writes elapsed={:?}", started.elapsed());
    let result = success(&home, &["search", "token LOAD-73"]);
    assert!(result.contains("LOAD-73"), "specific item lost: {result}");

    eprintln!("=== concurrent CLI writes ===");
    let mut children = Vec::new();
    for i in 1..=20 {
        let text = format!("Concurrent memory {i}, token CONCURRENT-{i}");
        let child = Command::new(mag())
            .args(["ingest", text.as_str()])
            .env("MAG_DATA_ROOT", home.path().join(".mag"))
            .env("HOME", home.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn concurrent mag");
        children.push(child);
    }
    let failures = children
        .into_iter()
        .filter(|child| !child.wait_with_output().expect("wait child").status.success())
        .count();
    eprintln!("concurrent failures={failures}/20");
    let result = success(&home, &["search", "CONCURRENT-17"]);
    assert!(result.contains("CONCURRENT-17"), "concurrent item missing: {result}");

    eprintln!("=== final health / reopen ===");
    let _ = run(&home, &["doctor"]);
    assert!(success(&home, &["search", "LOAD-73"]).contains("LOAD-73"));
}
