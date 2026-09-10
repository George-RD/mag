from pathlib import Path
import subprocess

p=Path('benches/runtime_behaviour/runner.rs'); s=p.read_text()
# Adapt the internal metadata call shape before adding behavior tests. The
# initial async body still blocks and must fail the executor-progress regression.
s=s.replace('fn persisted_embedding_space(db_path: &Path)', 'async fn persisted_embedding_space(db_path: &Path)')
old='''    let note_space = |path: &Path, space: &mut Option<String>| -> Result<()> {
        if space.is_none() {
            *space = Some(persisted_embedding_space(path)?);
        }
        Ok(())
    };
'''
assert old in s
s=s.replace(old,'')
s=s.replace('note_space(&db_path, &mut embedding_space)?;', 'note_space(&db_path, &mut embedding_space).await?;')
s=s.replace('note_space(&path, &mut embedding_space)?;', 'note_space(&path, &mut embedding_space).await?;')
s+='''
async fn note_space(path: &Path, space: &mut Option<String>) -> Result<()> {
    if space.is_none() {
        *space = Some(persisted_embedding_space(path).await?);
    }
    Ok(())
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use mag::memory_core::embedder::{Embedder, PlaceholderEmbedder};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    struct ExecutorGuard(std::thread::ThreadId);
    impl Embedder for ExecutorGuard {
        fn dimension(&self) -> usize {
            assert_ne!(std::thread::current().id(), self.0, "SQLite setup must not run on the async executor");
            PlaceholderEmbedder.dimension()
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            PlaceholderEmbedder.embed(text)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn review_sqlite_setup_leaves_the_executor() {
        let directory = tempfile::tempdir().unwrap();
        let backend = Backend::Placeholder(Arc::new(ExecutorGuard(std::thread::current().id())));
        let group = seed_group(&backend, directory.path().join("setup.db"), &[], NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).await.unwrap();
        assert_eq!(group.retained, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn review_locked_metadata_read_does_not_park_executor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("metadata.db");
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch("CREATE TABLE runtime_metadata (key TEXT PRIMARY KEY, value TEXT); INSERT INTO runtime_metadata VALUES ('embedding_space_identity', 'test-space'); BEGIN EXCLUSIVE;").unwrap();
        let (send_tick, receive_tick) = mpsc::channel();
        let release = std::thread::spawn(move || {
            let progressed = receive_tick.recv_timeout(Duration::from_secs(2)).is_ok();
            writer.execute_batch("COMMIT").unwrap();
            progressed
        });
        let (result, ()) = tokio::join!(persisted_embedding_space(&path), async move {
            tokio::task::yield_now().await;
            let _ = send_tick.send(());
        });
        assert!(release.join().unwrap(), "metadata read parked the async executor until the watchdog released SQLite");
        assert_eq!(result.unwrap(), "test-space");
    }
}
'''
p.write_text(s)
p=Path('tests/runtime_behaviour_eval.rs'); s=p.read_text(); s+='''
fn review_private_relative_command(equals_form: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = tempfile::tempdir().unwrap();
    let private = temporary.path().join("customer_secret_case_7391");
    std::fs::create_dir(&private).unwrap();
    for filename in ["dataset.json", "manifest.json"] {
        std::fs::copy(root.join("data/runtime_behaviour_eval/v1").join(filename), private.join(filename)).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_memory_runtime_eval"));
    command.current_dir(temporary.path()).args(["--validate-only", "--json"]);
    if equals_form {
        command.arg("--dataset=customer_secret_case_7391");
    } else {
        command.args(["--dataset", "customer_secret_case_7391"]);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("customer_secret_case_7391"), "metadata leaked a private relative dataset directory");
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["metadata"]["dataset_source"], "user-supplied");
    assert!(result["metadata"]["command"].as_str().unwrap().contains("--validate-only"));
}

#[test]
fn review_redacts_relative_dataset_split_option() {
    review_private_relative_command(false);
}

#[test]
fn review_redacts_relative_dataset_equals_option() {
    review_private_relative_command(true);
}

#[test]
fn review_rejects_abstention_with_positive_references() {
    let data = serde_json::from_slice::<Value>(include_bytes!("../data/runtime_behaviour_eval/v1/dataset.json")).unwrap();
    let id = data["questions"].as_array().unwrap().iter().find(|c| !c["relevant_keys"].as_array().unwrap().is_empty()).unwrap()["id"].as_str().unwrap().to_string();
    let out = mutated_validation(|data| {
        let case = data["questions"].as_array_mut().unwrap().iter_mut().find(|c| c["id"] == id).unwrap();
        case["expect_abstain"] = Value::Bool(true);
    });
    assert!(!out.status.success(), "contradictory abstention annotation was accepted");
}

#[test]
fn review_rejects_answerable_question_without_references() {
    let out = mutated_validation(|data| {
        let case = &mut data["questions"][0];
        case["expect_abstain"] = Value::Bool(false);
        case["relevant_keys"] = serde_json::json!([]);
    });
    assert!(!out.status.success(), "answerable question with no reference was accepted");
}
'''; p.write_text(s)
red=subprocess.run(['cargo','test','--locked','--no-default-features','--bin','memory_runtime_eval','review_tests'],capture_output=True,text=True)
print('EXECUTOR RED\n'+(red.stdout+red.stderr)[-9000:])
assert red.returncode != 0 and '2 failed' in red.stdout
red=subprocess.run(['cargo','test','--locked','--no-default-features','--test','runtime_behaviour_eval','review_'],capture_output=True,text=True)
print('PRIVACY/ANNOTATION RED\n'+(red.stdout+red.stderr)[-9000:])
assert red.returncode != 0 and '4 failed' in red.stdout

p=Path('benches/runtime_behaviour/backend.rs'); s=p.read_text().replace('pub enum Backend {', '#[derive(Clone)]\npub enum Backend {'); p.write_text(s)
p=Path('benches/runtime_behaviour/runner.rs'); s=p.read_text()
a=s.index('async fn persisted_embedding_space('); b=s.index('\n/// Converts a',a)
s=s[:a]+'''async fn persisted_embedding_space(db_path: &Path) -> Result<String> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).with_context(|| format!("failed to open {}", db_path.display()))?;
        conn.query_row(
            "SELECT value FROM runtime_metadata WHERE key = 'embedding_space_identity'",
            [], |row| row.get::<_, String>(0),
        ).context("database has no persisted embedding-space identity")
    }).await.context("embedding-space metadata task failed")?
}
''' + s[b:]
old='    let runtime = backend.open(db_path)?;'
assert old in s
s=s.replace(old, '''    let owned_backend = backend.clone();
    let runtime = tokio::task::spawn_blocking(move || owned_backend.open(db_path))
        .await.context("runtime initialization task failed")??;''')
p.write_text(s)
p=Path('benches/runtime_behaviour/main.rs'); s=p.read_text().replace('let metadata = benchmarking::benchmark_metadata_from_parts(', 'let mut metadata = benchmarking::benchmark_metadata_from_parts(')
needle='''        &args.dataset.join("dataset.json").to_string_lossy(),
    );'''
assert needle in s
s=s.replace(needle,needle+'''\n    // Record typed options rather than the original argv: a bare relative path
    // has no slash and is not recognized by the shared generic sanitizer.
    metadata.command = recorded_command(&args)?;''')
s+='''
/// Canonical option representation, not the original argv or a replay script.
fn recorded_command(args: &Args) -> Result<String> {
    let embedder = match args.embedder {
        EmbedderChoice::Placeholder => "placeholder",
        EmbedderChoice::BgeSmall => "bge-small",
    };
    let mut command = format!("memory_runtime_eval --dataset '<redacted_path>' --embedder {embedder}");
    for family in &args.family {
        command.push_str(" --family ");
        command.push_str(&serde_json::to_string(family)?);
    }
    for (enabled, flag) in [(args.json, "--json"), (args.validate_only, "--validate-only"), (args.quiet, "--quiet")] {
        if enabled {
            command.push(' ');
            command.push_str(flag);
        }
    }
    Ok(command)
}
'''; p.write_text(s)
p=Path('benches/runtime_behaviour/dataset/validation.rs'); s=p.read_text()
needle='''    for c in &data.questions {
        for key in &c.relevant_keys {'''
assert needle in s
s=s.replace(needle,'''    for c in &data.questions {
        if c.expect_abstain != c.relevant_keys.is_empty() {
            failures.push(format!("question {} has inconsistent abstention and relevant_keys", c.id));
        }
        for key in &c.relevant_keys {''')
p.write_text(s)
p=Path('benches/runtime_behaviour/README.md'); p.write_text(p.read_text()+'''\nDatabase initialization and persisted-identity reads are isolated in Tokio blocking\ntasks. Report command metadata is a canonical representation of typed options,\nnot exact argv; dataset directories are redacted even for bare relative names\nand equals-form arguments. An answerable question must have relevant keys, and\nan abstention question must have none. Contradictions fail before model startup.\n''')
p=Path('meta/reviews/runtime-behaviour-recovery.md'); p.write_text(p.read_text()+'''\n## Independent review follow-up

Codex 3977722805 identified SQLite initialization inside async seeding. Two
current-thread regressions cover actual initialization and a locked metadata
read; the latter lets executor progress release a real SQLite transaction.
Both fail before moving initialization and metadata reads to spawn_blocking.
Join errors retain context. The runtime itself is unchanged.

Codex 3977722810 found a bare-relative-path hole in generic command metadata.
Split and equals-form process regressions fail before this diagnostic records
canonical typed options with the dataset redacted. Shared provenance fields and
the dataset digest remain; no production helper is changed.

Codex 3977722814 found contradictory question annotations. Two process tests
fail before validation requires references for answerable questions and none for
abstention controls. The original dataset and historical observation stay
byte-identical. The earlier edge-direction finding was independently fixed in
47a33b6 with two runtime-backed RED/GREEN tests.
''')
