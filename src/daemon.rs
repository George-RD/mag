use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Information about a running MAG daemon, written to `~/.mag/daemon.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub port: u16,
    pub pid: u32,
    pub version: String,
    pub token: String,
}

impl DaemonInfo {
    /// Returns the path to `daemon.json` inside the MAG data directory (`~/.mag/daemon.json`).
    pub fn path() -> Result<PathBuf> {
        let paths = crate::app_paths::resolve_app_paths()?;
        Ok(paths.data_root.join("daemon.json"))
    }

    /// Atomically writes this `DaemonInfo` to `daemon.json`.
    ///
    /// Serializes to JSON, writes to a `.tmp` sibling, then renames into place.
    pub fn write(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let tmp_path = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).context("serializing DaemonInfo")?;
        std::fs::write(&tmp_path, json)
            .with_context(|| format!("writing {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))?;
        Ok(())
    }

    /// Reads and parses `daemon.json`. Returns `Ok(None)` if the file does not exist.
    pub fn read() -> Result<Option<DaemonInfo>> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let info: DaemonInfo = serde_json::from_str(&contents)
                    .with_context(|| format!("parsing {}", path.display()))?;
                Ok(Some(info))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::anyhow!(e).context(format!("reading {}", path.display()))),
        }
    }

    /// Removes `daemon.json` if it exists. Errors are silently ignored.
    pub fn remove() {
        if let Ok(path) = Self::path() {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Returns `true` if the process identified by `self.pid` is no longer alive.
    ///
    /// On Unix this sends signal 0 via `kill(2)`. On Windows we cannot cheaply
    /// check liveness, so we conservatively return `false` (assume not stale).
    pub fn is_stale(&self) -> bool {
        #[cfg(unix)]
        {
            let Ok(status) = std::process::Command::new("kill")
                .args(["-0", &self.pid.to_string()])
                .stderr(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .status()
            else {
                return true;
            };
            !status.success()
        }
        #[cfg(not(unix))]
        {
            // On Windows (and other non-Unix platforms), we cannot reliably
            // probe process liveness without platform-specific APIs. Default
            // to "not stale" so the caller will attempt a health-check instead.
            false
        }
    }
}

/// Generates a 32-byte random hex auth token (64 hex characters).
pub fn generate_auth_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
}

/// Reads the auth token from `~/.mag/auth.token`, creating one if it doesn't exist.
///
/// On Unix the file is created with mode 0600 (owner read/write only).
pub fn read_or_create_auth_token() -> Result<String> {
    let paths = crate::app_paths::resolve_app_paths()?;
    let token_path = paths.data_root.join("auth.token");

    // Return the existing token if the file exists and is non-empty.
    match std::fs::read_to_string(&token_path) {
        Ok(contents) => {
            let existing = contents.trim();
            if !existing.is_empty() {
                return Ok(existing.to_string());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(anyhow::anyhow!(e).context(format!("reading {}", token_path.display())));
        }
    }

    // File missing or empty -- generate a fresh token and persist it.
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let token = generate_auth_token();
    write_token_file(&token_path, &token)?;
    Ok(token)
}

/// Writes a token to the given path with restrictive permissions on Unix.
///
/// On Unix the file is created atomically with mode 0600 via `OpenOptionsExt::mode()`,
/// avoiding a TOCTOU window where the file briefly exists with default permissions.
fn write_token_file(path: &std::path::Path, token: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("creating {}", path.display()))?;
        file.write_all(token.as_bytes())
            .with_context(|| format!("writing {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, token).with_context(|| format!("writing {}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mutex to serialize tests that mutate the `HOME` env var.
    static HOME_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper: set HOME to a temp dir so `DaemonInfo::path()` resolves there.
    ///
    /// Holds `HOME_MUTEX` for the duration so parallel tests don't race on the
    /// shared environment variable.
    fn with_temp_home(f: impl FnOnce(PathBuf)) {
        let _guard = HOME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("mag-daemon-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".mag")).unwrap();
        let prev = std::env::var_os("HOME");
        // SAFETY: protected by HOME_MUTEX; no other test mutates HOME concurrently.
        unsafe { std::env::set_var("HOME", &dir) };
        f(dir.clone());
        match prev {
            Some(val) => unsafe { std::env::set_var("HOME", val) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_read_round_trip() {
        with_temp_home(|_dir| {
            let info = DaemonInfo {
                port: 8080,
                pid: 12345,
                version: "0.1.0".to_string(),
                token: "abc123".to_string(),
            };
            info.write().unwrap();
            let read_back = DaemonInfo::read()
                .unwrap()
                .expect("should find daemon.json");
            assert_eq!(read_back.port, 8080);
            assert_eq!(read_back.pid, 12345);
            assert_eq!(read_back.version, "0.1.0");
            assert_eq!(read_back.token, "abc123");
        });
    }

    #[test]
    fn read_returns_none_when_missing() {
        with_temp_home(|_dir| {
            let result = DaemonInfo::read().unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn remove_deletes_file() {
        with_temp_home(|_dir| {
            let info = DaemonInfo {
                port: 9090,
                pid: 1,
                version: "0.0.1".to_string(),
                token: "tok".to_string(),
            };
            info.write().unwrap();
            assert!(DaemonInfo::read().unwrap().is_some());
            DaemonInfo::remove();
            assert!(DaemonInfo::read().unwrap().is_none());
        });
    }

    #[test]
    fn is_stale_for_nonexistent_pid() {
        let info = DaemonInfo {
            port: 1234,
            pid: 999_999,
            version: "0.0.0".to_string(),
            token: String::new(),
        };
        assert!(info.is_stale());
    }

    #[test]
    fn generate_auth_token_is_64_hex_chars() {
        let token = generate_auth_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn read_or_create_auth_token_creates_and_reads() {
        with_temp_home(|dir| {
            let token1 = read_or_create_auth_token().unwrap();
            assert_eq!(token1.len(), 64);

            // Second call should return the same token
            let token2 = read_or_create_auth_token().unwrap();
            assert_eq!(token1, token2);

            // Verify file exists with restrictive perms on Unix
            let token_path = dir.join(".mag/auth.token");
            assert!(token_path.exists());

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::metadata(&token_path).unwrap().permissions();
                assert_eq!(perms.mode() & 0o777, 0o600);
            }
        });
    }
}
