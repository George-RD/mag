use anyhow::Result;

// ── Peak RSS tracking ─────────────────────────────────────────────────────

/// Largest resident-set size observed across `sample()` calls.
///
/// On Linux each sample reads `VmHWM`, the kernel's high-water mark, so
/// `peak_kb` is the process peak even if the peak fell between two samples. On
/// macOS `ps` reports current RSS, so `peak_kb` is a sampled maximum there.
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

/// Reads the kernel's own high-water mark (`VmHWM`) so a sample taken at a
/// quiet moment still reports the peak reached between samples. Falls back to
/// current `VmRSS` on a kernel that does not publish `VmHWM`.
#[cfg(target_os = "linux")]
fn current_rss_kb() -> Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let mut current = None;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmHWM:") {
            return Ok(value.trim().trim_end_matches(" kB").trim().parse()?);
        }
        if let Some(value) = line.strip_prefix("VmRSS:") {
            current = Some(value.trim().trim_end_matches(" kB").trim().parse::<u64>()?);
        }
    }
    Ok(current.unwrap_or(0))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_rss_kb() -> Result<u64> {
    Ok(0)
}
