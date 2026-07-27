//! Temporary black-box product trial. Do not merge.

use std::process::Command;
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
    eprintln!("\n$ mag {}\nexit={code}\nstdout:\n{stdout}\nstderr:\n{stderr}", args.join(" "));
    (code, stdout, stderr)
}

fn ok(home: &TempDir, args: &[&str]) -> String {
    let (code, stdout, stderr) = run(home, args);
    assert_eq!(code, 0, "command failed: {args:?}\n{stderr}");
    stdout
}

#[test]
fn black_box_product_trial() {
    let home = TempDir::new().unwrap();

    eprintln!("=== discoverability ===");
    ok(&home, &["--version"]);
    ok(&home, &["--help"]);
    let _ = run(&home, &["doctor"]);

    eprintln!("=== exact and semantic recall ===");
    ok(&home, &["ingest", "The deployment region for Project Atlas is eu-central-1."]);
    let result = ok(&home, &["search", "Where is Atlas deployed?"]);
    assert!(result.contains("eu-central-1"), "exact fact missed: {result}");

    ok(&home, &["ingest", "Retries must use exponential backoff with random jitter and stop after five attempts."]);
    let result = ok(&home, &["search", "What is our policy when requests keep failing?"]);
    assert!(result.to_lowercase().contains("backoff") || result.to_lowercase().contains("jitter"), "paraphrase recall failed: {result}");

    eprintln!("=== persistence and duplicate ===");
    assert!(ok(&home, &["search", "Atlas deployment region"]).contains("eu-central-1"));
    ok(&home, &["ingest", "The deployment region for Project Atlas is eu-central-1."]);
    eprintln!("duplicate result: {}", ok(&home, &["search", "Atlas region"]));

    eprintln!("=== conflicting update ===");
    ok(&home, &["ingest", "Project Atlas moved from eu-central-1 to me-central-1. The current deployment region is me-central-1."]);
    let result = ok(&home, &["search", "What is the current deployment region for Project Atlas?"]);
    eprintln!("conflict result: {result}");
    assert!(result.contains("me-central-1"), "new fact not surfaced: {result}");

    eprintln!("=== distractor resistance ===");
    ok(&home, &["ingest", "Project Borealis uses PostgreSQL, not MySQL."]);
    ok(&home, &["ingest", "A discarded prototype once used MySQL for an unrelated demo."]);
    let result = ok(&home, &["search", "Which database does Borealis use?"]);
    assert!(result.to_lowercase().contains("postgres"), "distractor displaced fact: {result}");

    eprintln!("=== unicode and multiline ===");
    ok(&home, &["ingest", "Client note: café access code is ٤٢.\nOwner: Zoë.\nDo not normalize identifier A/B-17."]);
    let result = ok(&home, &["search", "Who owns the cafe access note and which identifier must remain unchanged?"]);
    assert!(result.contains("Zoë") || result.contains("A/B-17"), "unicode recall failed: {result}");

    eprintln!("=== empty inputs ===");
    let empty_ingest = run(&home, &["ingest", ""]);
    let empty_search = run(&home, &["search", ""]);
    let whitespace_search = run(&home, &["search", "   "]);
    eprintln!("empty exits: ingest={} search={} whitespace={}", empty_ingest.0, empty_search.0, whitespace_search.0);

    eprintln!("=== large input ===");
    let large = format!("Large-context sentinel MAG-LARGE-9472. {}", "alpha beta gamma delta ".repeat(20_000));
    let started = Instant::now();
    let large_result = run(&home, &["ingest", &large]);
    assert!(started.elapsed() < Duration::from_secs(60));
    eprintln!("large input exit={}", large_result.0);
    if large_result.0 == 0 {
        assert!(ok(&home, &["search", "What is the large-context sentinel?"]).contains("MAG-LARGE-9472"));
    }

    eprintln!("=== rapid sequential writes ===");
    let started = Instant::now();
    for i in 1..=100 {
        let text = format!("Load-test memory item {i}, token LOAD-{i}");
        ok(&home, &["ingest", &text]);
    }
    eprintln!("100 writes elapsed={:?}", started.elapsed());
    assert!(ok(&home, &["search", "token LOAD-73"]).contains("LOAD-73"));

    eprintln!("=== final health ===");
    let _ = run(&home, &["doctor"]);
}
