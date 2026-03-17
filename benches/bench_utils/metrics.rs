use anyhow::Result;

// ── Peak RSS tracking ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct PeakRss {
    pub peak_kb: u64,
}

impl PeakRss {
    pub fn sample(&mut self) {
        if let Ok(kb) = current_rss_kb()
            && kb > self.peak_kb
        {
            self.peak_kb = kb;
        }
    }
}

#[cfg(target_os = "macos")]
fn current_rss_kb() -> Result<u64> {
    let pid = std::process::id();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.trim().parse()?)
}

#[cfg(target_os = "linux")]
fn current_rss_kb() -> Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            let kb: u64 = value.trim().trim_end_matches(" kB").trim().parse()?;
            return Ok(kb);
        }
    }
    Ok(0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_rss_kb() -> Result<u64> {
    Ok(0)
}
