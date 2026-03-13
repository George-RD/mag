use std::process::Command;

#[test]
fn parity_harness_scaffold_runs_mag_and_optionally_omega() -> anyhow::Result<()> {
    let output = Command::new("bash")
        .arg("parity/run_parity.sh")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "parity harness failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("mag ingest:"));

    Ok(())
}
