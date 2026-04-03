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

pub fn resolve_app_paths() -> Result<AppPaths> {
    Ok(app_paths_for_home(&home_dir()?))
}

fn app_paths_for_home(home: &Path) -> AppPaths {
    let data_root = home.join(APP_DIR);

    AppPaths {
        home_dir: home.to_path_buf(),
        data_root: data_root.clone(),
        database_path: data_root.join("memory.db"),
        model_root: data_root.join("models"),
        benchmark_root: data_root.join("benchmarks"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_mag_root() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", uuid::Uuid::new_v4()));
        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".mag"));
        assert_eq!(paths.database_path, home.join(".mag/memory.db"));
        assert_eq!(paths.model_root, home.join(".mag/models"));
        assert_eq!(paths.benchmark_root, home.join(".mag/benchmarks"));
    }
}
