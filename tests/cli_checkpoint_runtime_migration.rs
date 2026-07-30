use std::path::Path;
use std::process::{Command, Output};

const TASK_TITLE: &str = "runtime checkpoint migration";
const PROGRESS: &str = "checkpoint caller contract pinned";
const PLAN: &str = "keep session continuity behind the local runtime";
const NEXT_STEPS: &str = "migrate the remaining session commands";
const PROJECT: &str = "runtime-migration";
const SESSION_ID: &str = "checkpoint-session";

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

#[test]
fn checkpoint_commands_use_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;

    let checkpoint = run_cli(
        home.path(),
        &[
            "checkpoint",
            TASK_TITLE,
            PROGRESS,
            "--plan",
            PLAN,
            "--next-steps",
            NEXT_STEPS,
            "--session-id",
            SESSION_ID,
            "--project",
            PROJECT,
        ],
    )?;
    let checkpoint_stdout = String::from_utf8(checkpoint.stdout)?;
    let checkpoint_stderr = String::from_utf8(checkpoint.stderr)?;
    let checkpoint_payload: serde_json::Value = serde_json::from_str(checkpoint_stdout.trim())?;
    let memory_id = checkpoint_payload["memory_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("checkpoint output omitted memory_id"))?;
    let expected_checkpoint = serde_json::json!({
        "memory_id": memory_id,
        "checkpoint_number": 1
    });
    assert_eq!(checkpoint_stdout, format!("{expected_checkpoint}\n"));
    assert!(
        checkpoint_stderr.contains("Checkpoint saved through local memory runtime"),
        "checkpoint did not report the selected runtime path: {checkpoint_stderr}"
    );

    let resume = run_cli(
        home.path(),
        &[
            "resume-task",
            "--task-title",
            TASK_TITLE,
            "--project",
            PROJECT,
            "--limit",
            "1",
        ],
    )?;
    let resume_stdout = String::from_utf8(resume.stdout)?;
    let resume_stderr = String::from_utf8(resume.stderr)?;
    let resume_without_newline = resume_stdout
        .strip_suffix('\n')
        .ok_or_else(|| anyhow::anyhow!("resume-task output must end with a newline"))?;
    let (resume_prefix, created_at) = resume_without_newline
        .rsplit_once("\n\nCreated At: ")
        .ok_or_else(|| anyhow::anyhow!("resume-task output omitted Created At"))?;
    chrono::DateTime::parse_from_rfc3339(created_at)?;

    let content = format!(
        "## Checkpoint: {TASK_TITLE}\n### Progress\n{PROGRESS}\n\n### Plan\n{PLAN}\n\n### Next Steps\n{NEXT_STEPS}"
    );
    let metadata = serde_json::json!({
        "checkpoint_number": 1,
        "checkpoint_data": {
            "task_title": TASK_TITLE,
            "plan": PLAN,
            "progress": PROGRESS,
            "files_touched": null,
            "decisions": null,
            "key_context": null,
            "next_steps": NEXT_STEPS
        }
    });
    let expected_prefix =
        format!("### Checkpoint\n{content}\n\nMetadata:\n{metadata}");
    assert_eq!(resume_prefix, expected_prefix);
    assert_eq!(
        resume_stdout,
        format!("{expected_prefix}\n\nCreated At: {created_at}\n")
    );
    assert!(
        resume_stderr.contains("Checkpoint task resumed through local memory runtime"),
        "resume-task did not report the selected runtime path: {resume_stderr}"
    );

    Ok(())
}
