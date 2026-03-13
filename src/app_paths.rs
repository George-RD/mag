use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
const APP_DIR: &str = ".mag";
const LEGACY_APP_DIR: &str = ".romega-memory";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub home_dir: PathBuf,
    pub preferred_data_root: PathBuf,
    pub data_root: PathBuf,
    pub legacy_data_root: PathBuf,
    pub using_legacy_root: bool,
    pub database_path: PathBuf,
    pub model_root: PathBuf,
    pub benchmark_root: PathBuf,
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
    let preferred = home.join(APP_DIR);
    let legacy = home.join(LEGACY_APP_DIR);
    let use_legacy = !has_runtime_db(&preferred) && has_runtime_db(&legacy);
    let data_root = if use_legacy {
        legacy.clone()
    } else {
        preferred
    };

    AppPaths {
        home_dir: home.to_path_buf(),
        preferred_data_root: home.join(APP_DIR),
        data_root: data_root.clone(),
        legacy_data_root: legacy,
        using_legacy_root: use_legacy,
        database_path: data_root.join("memory.db"),
        model_root: data_root.join("models"),
        benchmark_root: data_root.join("benchmarks"),
    }
}

fn has_runtime_db(root: &Path) -> bool {
    root.join("memory.db").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn new_install_prefers_mag_root() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", Uuid::new_v4()));
        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".mag"));
        assert!(!paths.using_legacy_root);
    }

    #[test]
    fn legacy_root_is_used_when_mag_root_is_absent() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".romega-memory")).unwrap();
        std::fs::write(home.join(".romega-memory").join("memory.db"), []).unwrap();

        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".romega-memory"));
        assert!(paths.using_legacy_root);

        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn mag_root_wins_when_both_roots_exist() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".mag")).unwrap();
        std::fs::create_dir_all(home.join(".romega-memory")).unwrap();

        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".mag"));
        assert!(!paths.using_legacy_root);

        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn empty_mag_root_does_not_block_populated_legacy_root() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".mag")).unwrap();
        std::fs::create_dir_all(home.join(".romega-memory")).unwrap();
        std::fs::write(home.join(".romega-memory").join("memory.db"), []).unwrap();

        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".romega-memory"));
        assert!(paths.using_legacy_root);

        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn mag_cache_dirs_do_not_block_populated_legacy_root() {
        let home = std::env::temp_dir().join(format!("mag-paths-{}", Uuid::new_v4()));
        std::fs::create_dir_all(home.join(".mag").join("models")).unwrap();
        std::fs::create_dir_all(home.join(".romega-memory")).unwrap();
        std::fs::write(home.join(".romega-memory").join("memory.db"), []).unwrap();

        let paths = app_paths_for_home(&home);
        assert_eq!(paths.data_root, home.join(".romega-memory"));
        assert!(paths.using_legacy_root);

        let _ = std::fs::remove_dir_all(home);
    }
}
