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

#[test]
fn traverse_command_uses_local_runtime_without_contract_drift() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!(
        "mag-cli-traverse-runtime-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&home)?;

    let traversal = run_cli(&home, &["traverse", "missing-memory"])?;
    let stdout = String::from_utf8(traversal.stdout)?;
    let stderr = String::from_utf8(traversal.stderr)?;
    assert_eq!(stdout, "{}\n");
    assert!(
        stderr.contains("Graph traversal completed through local memory runtime"),
        "traverse did not report the selected runtime path: {stderr}"
    );

    std::fs::remove_dir_all(home)?;
    Ok(())
}
