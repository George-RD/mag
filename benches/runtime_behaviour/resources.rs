use anyhow::Result;
use std::path::PathBuf;

#[derive(Default)]
pub struct PeakRss {
    pub peak_kb: Option<u64>,
}
impl PeakRss {
    pub fn sample(&mut self) {
        if let Some(value) = sample_rss() {
            self.peak_kb = Some(self.peak_kb.map_or(value, |previous| previous.max(value)));
        }
    }
}
#[cfg(any(target_os = "linux", test))]
fn high_water(status: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}
#[cfg(target_os = "linux")]
fn sample_rss() -> Option<u64> {
    high_water(&std::fs::read_to_string("/proc/self/status").ok()?)
}
#[cfg(target_os = "macos")]
fn sample_rss() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn sample_rss() -> Option<u64> {
    None
}
pub fn method() -> &'static str {
    if cfg!(target_os = "linux") {
        "Linux VmHWM process high-water mark; null when unavailable"
    } else if cfg!(target_os = "macos") {
        "sampled current RSS maximum, not a kernel high-water mark"
    } else {
        "unavailable on this platform"
    }
}

/// A newly created private directory, never a reused PID-derived database.
pub struct Workspace(pub PathBuf);
impl Workspace {
    pub fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("mag-runtime-eval-{}", uuid::Uuid::new_v4()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_ram_is_not_zero() {
        assert_eq!(high_water("VmRSS: 12 kB\n"), None);
        assert_eq!(high_water("VmHWM: 42 kB\n"), Some(42));
        assert_eq!(high_water("VmHWM: unknown kB\n"), None);
    }
    #[test]
    fn workspaces_are_unique_and_cleaned() {
        let first = Workspace::new().unwrap();
        let second = Workspace::new().unwrap();
        assert_ne!(first.0, second.0);
        let path = first.0.clone();
        drop(first);
        assert!(!path.exists());
        assert!(second.0.is_dir());
    }
}
