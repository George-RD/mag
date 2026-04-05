use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
const APP_DIR: &str = ".mag";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub home_dir: PathBuf,
    pub data_root: PathBuf,
    pub database_path: PathBuf,
    pub model_root: PathBuf,
    pub benchmark_root: PathBuf,
}

/// Returns the XDG config home directory, respecting `XDG_CONFIG_HOME` if set
/// and absolute, otherwise falling back to `$HOME/.config`.
#[allow(dead_code)] // used by setup and tool_detection; not reachable from the binary target
pub fn xdg_config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("neither HOME nor USERPROFILE is set"))
}

/// Resolves the MAG data root directory.
///
/// If `MAG_DATA_ROOT` is set, it must be an absolute path; a relative path is
/// rejected with an error. When the variable is unset the default `$HOME/.mag`
/// is used.
pub fn resolve_data_root(home: &Path) -> Result<PathBuf> {
    match std::env::var_os("MAG_DATA_ROOT") {
        Some(val) => {
            let path = PathBuf::from(val);
            if path.is_absolute() {
                Ok(path)
            } else {
                Err(anyhow!(
                    "MAG_DATA_ROOT must be an absolute path, got: {}",
                    path.display()
                ))
            }
        }
        None => Ok(home.join(APP_DIR)),
    }
}

pub fn resolve_app_paths() -> Result<AppPaths> {
    let home = home_dir()?;
    let data_root = resolve_data_root(&home)?;
    Ok(app_paths_for(home, data_root))
}

fn app_paths_for(home: PathBuf, data_root: PathBuf) -> AppPaths {
    AppPaths {
        database_path: data_root.join("memory.db"),
        model_root: data_root.join("models"),
        benchmark_root: data_root.join("benchmarks"),
        home_dir: home,
        data_root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_mag_root() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", uuid::Uuid::new_v4()));
        let data_root = home.join(".mag");
        let paths = app_paths_for(home.clone(), data_root.clone());
        assert_eq!(paths.data_root, home.join(".mag"));
        assert_eq!(paths.database_path, home.join(".mag/memory.db"));
        assert_eq!(paths.model_root, home.join(".mag/models"));
        assert_eq!(paths.benchmark_root, home.join(".mag/benchmarks"));
    }

    #[test]
    fn mag_data_root_override_absolute_path() {
        let override_dir =
            std::env::temp_dir().join(format!("mag-override-{}", uuid::Uuid::new_v4()));
        // Serialize with all other tests that mutate env vars (HOME, XDG_CONFIG_HOME, MAG_DATA_ROOT).
        let _guard = crate::test_helpers::HOME_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("MAG_DATA_ROOT");
        // SAFETY: Serialized by HOME_MUTEX; no other test mutates MAG_DATA_ROOT concurrently.
        unsafe { std::env::set_var("MAG_DATA_ROOT", &override_dir) };
        let home = PathBuf::from("/home/testuser");
        let result = resolve_data_root(&home);
        // Restore
        unsafe {
            match prev {
                Some(v) => std::env::set_var("MAG_DATA_ROOT", v),
                None => std::env::remove_var("MAG_DATA_ROOT"),
            }
        }
        let data_root = result.expect("absolute MAG_DATA_ROOT should be accepted");
        assert_eq!(data_root, override_dir);
    }

    #[test]
    fn mag_data_root_fallback_to_home_dot_mag() {
        let _guard = crate::test_helpers::HOME_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("MAG_DATA_ROOT");
        // SAFETY: Serialized by HOME_MUTEX; no other test mutates MAG_DATA_ROOT concurrently.
        unsafe { std::env::remove_var("MAG_DATA_ROOT") };
        let home = PathBuf::from("/home/testuser");
        let result = resolve_data_root(&home);
        // Restore
        unsafe {
            if let Some(v) = prev {
                std::env::set_var("MAG_DATA_ROOT", v);
            }
        }
        assert_eq!(result.unwrap(), PathBuf::from("/home/testuser/.mag"));
    }

    #[test]
    fn mag_data_root_rejects_relative_path() {
        let _guard = crate::test_helpers::HOME_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os("MAG_DATA_ROOT");
        // SAFETY: Serialized by HOME_MUTEX; no other test mutates MAG_DATA_ROOT concurrently.
        unsafe { std::env::set_var("MAG_DATA_ROOT", "relative/path") };
        let home = PathBuf::from("/home/testuser");
        let result = resolve_data_root(&home);
        // Restore
        unsafe {
            match prev {
                Some(v) => std::env::set_var("MAG_DATA_ROOT", v),
                None => std::env::remove_var("MAG_DATA_ROOT"),
            }
        }
        assert!(result.is_err(), "relative MAG_DATA_ROOT should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("absolute"), "error should mention 'absolute'");
    }
}
