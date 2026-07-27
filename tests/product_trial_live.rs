//! Temporary black-box product trial. Do not merge.

use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use serde_json::{json, Value};

fn mag() -> &'static str { env!("CARGO_BIN_EXE_mag") }

fn run(home: &TempDir, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(mag())
        .args(args)
        .env("MAG_DATA_ROOT", home.path().join(".mag"))
        .env("HOME", home.path())
        .output()
        .expect("spawn mag");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("\n$ mag {}\nexit={code}\nstdout:\n{stdout}\nstderr:\n{stderr}", args.join(" "));
    (code, stdout, stderr)
}

fn assert_success(home: &TempDir, args: &[&str]) -> String {
    let (code, stdout, stderr) = run(home, args);
    assert_eq!(code, 0, "command failed: {args:?}\n{stderr}");
    stdout
}

#[test]
fn black_box_product_trial() {
    let home = TempDir::new().unwrap();

    eprintln!("=== clean state / discoverability ===");
    assert_success(&home, &["--version"]);
    assert_success(&home, &["--help"]);
    let _ = run(&home, &["doctor"]);

    eprintln!("=== basic semantic recall ===");
    assert_success(&home, &["ingest", "The deployment region for Project Atlas is eu-central-1."]);
    let s = assert_success(&home, &["search", "Where is Atlas deployed?"]);
    assert!(s.to_lowercase().contains("eu-central-1"), "semantic recall missed exact fact: {s}");

    assert_success(&home, &["ingest", "Retries must use exponential backoff with random jitter and stop after five attempts."]);
    let s = assert_success(&home, &["search", "What is our policy when requests keep failing?"]);
    assert!(s.to_lowercase().contains("backoff") || s.to_lowercase().contains("jitter"), "paraphrase recall failed: {s}");

    eprintln!("=== persistence across fresh processes ===");
    let s = assert_success(&home, &["search", "Atlas deployment region"]);
    assert!(s.contains("eu-central-1"), "process restart persistence failed: {s}");

    eprintln!("=== duplicate behavior ===");
    assert_success(&home, &["ingest", "The deployment region for Project Atlas is eu-central-1."]);
    let s = assert_success(&home, &["search", "Atlas region"]);
    eprintln!("duplicate search result: {s}");

    eprintln!("=== conflicting / updated memory ===");
    assert_success(&home, &["ingest", "Project Atlas has moved from eu-central-1 to me-central-1. The current deployment region is me-central-1."]);
    let s = assert_success(&home, &["search", "What is the current deployment region for Project Atlas?"]);
    eprintln!("conflict result: {s}");
    assert!(s.contains("me-central-1"), "new fact not surfaced: {s}");

    eprintln!("=== negation and distractor ===");
    assert_success(&home, &["ingest", "Project Borealis uses PostgreSQL, not MySQL."]);
    assert_success(&home, &["ingest", "A discarded prototype once used MySQL for an unrelated demo."]);
    let s = assert_success(&home, &["search", "Which database does Borealis use?"]);
    assert!(s.to_lowercase().contains("postgres"), "distractor displaced correct fact: {s}");

    eprintln!("=== unicode / multiline ===");
    assert_success(&home, &["ingest", "Client note: café access code is ٤٢.\nOwner: Zoë.\nDo not normalize identifier A/B-17."]);
    let s = assert_success(&home, &["search", "Who owns the cafe access note and which identifier must remain unchanged?"]);
    assert!(s.contains("Zoë") || s.contains("A/B-17"), "unicode/multiline recall failed: {s}");

    eprintln!("=== malformed and empty input ===");
    let empty_ingest = run(&home, &["ingest", ""]);
    let empty_search = run(&home, &["search", ""]);
    let whitespace_search = run(&home, &["search", "   "]);
    eprintln!("empty exits: ingest={} search={} whitespace={}", empty_ingest.0, empty_search.0, whitespace_search.0);

    eprintln!("=== oversized input behavior ===");
    let large = format!("Large-context sentinel MAG-LARGE-9472. {}", "alpha beta gamma delta ".repeat(20_000));
    let started = Instant::now();
    let large_result = run(&home, &["ingest", &large]);
    assert!(started.elapsed() < Duration::from_secs(60), "large ingest exceeded 60 seconds");
    eprintln!("large ingest exit={}", large_result.0);
    if large_result.0 == 0 {
        let s = assert_success(&home, &["search", "What is the large-context sentinel?"]);
        assert!(s.contains("MAG-LARGE-9472"), "large input stored but not recalled: {s}");
    }

    eprintln!("=== rapid sequential writes ===");
    let started = Instant::now();
    for i in 1..=100 {
        let text = format!("Load-test memory item {i}, token LOAD-{i}");
        assert_success(&home, &["ingest", &text]);
    }
    eprintln!("100 writes elapsed={:?}", started.elapsed());
    let s = assert_success(&home, &["search", "token LOAD-73"]);
    assert!(s.contains("LOAD-73"), "specific item lost after load: {s}");

    eprintln!("=== concurrent independent CLI writes ===");
    let mut children = Vec::new();
    for i in 1..=20 {
        let child = Command::new(mag())
            .args(["ingest", &format!("Concurrent memory {i}, token CONCURRENT-{i}")])
            .env("MAG_DATA_ROOT", home.path().join(".mag"))
            .env("HOME", home.path())
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        children.push(child);
    }
    let mut failures = 0;
    for child in children { if !child.wait_with_output().unwrap().status.success() { failures += 1; } }
    eprintln!("concurrent failures={failures}/20");
    let s = assert_success(&home, &["search", "CONCURRENT-17"]);
    assert!(s.contains("CONCURRENT-17"), "concurrent target not found: {s}");

    eprintln!("=== real MCP session ===");
    let mut child = Command::new(mag())
        .arg("serve")
        .env("MAG_DATA_ROOT", home.path().join(".mag"))
        .env("HOME", home.path())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut rpc = |v: Value| -> Value {
        writeln!(stdin, "{}", v).unwrap(); stdin.flush().unwrap();
        loop {
            let mut line = String::new(); reader.read_line(&mut line).unwrap();
            assert!(!line.is_empty(), "MCP server closed stdout");
            if let Ok(v) = serde_json::from_str::<Value>(&line) { if v.get("id").is_some() { eprintln!("MCP: {v}"); return v; } }
        }
    };
    let init = rpc(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"product-trial","version":"1"}}}));
    assert!(init.get("result").is_some());
    writeln!(stdin, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap(); stdin.flush().unwrap();
    let tools = rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let count = tools["result"]["tools"].as_array().map_or(0, |a| a.len());
    eprintln!("MCP tool count={count}");
    assert!(count >= 2);
    let stored = rpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_store","arguments":{"content":"MCP-only sentinel is MCP-8821","importance":0.9}}}));
    assert!(stored.get("result").is_some());
    let found = rpc(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"What is the MCP-only sentinel?"}}}));
    assert!(found.to_string().contains("MCP-8821"), "MCP write/read failed: {found}");
    let unknown = rpc(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"definitely_not_a_tool","arguments":{}}}));
    assert!(unknown.get("error").is_some() || unknown.to_string().to_lowercase().contains("unknown"));
    drop(stdin);
    let _ = child.kill();

    eprintln!("=== final reopen ===");
    let s = assert_success(&home, &["search", "MCP-8821"]);
    assert!(s.contains("MCP-8821"));
}
