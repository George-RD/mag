//! Comprehensive uninstall command for MAG.
//!
//! Removes tool configurations, downloaded models, the database, and
//! the `~/.mag` data directory. Supports interactive, `--all`, and
//! `--configs-only` modes.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::app_paths;
use crate::config_writer::{self, RemoveResult};
use crate::tool_detection;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Runs the uninstall flow.
///
/// * `all` — skip prompts and remove everything including the database.
/// * `configs_only` — skip prompts and only remove tool configurations.
pub async fn run_uninstall(all: bool, configs_only: bool) -> Result<()> {
    let paths = app_paths::resolve_app_paths()?;

    let mut choices = if configs_only {
        UninstallChoices {
            tool_configs: true,
            models: false,
            database: false,
        }
    } else if all || is_non_interactive() {
        UninstallChoices {
            tool_configs: true,
            models: true,
            database: all, // non-TTY without --all refuses database deletion
        }
    } else {
        prompt_choices(&paths)?
    };

    if choices.database && !all && !confirm_database_deletion()? {
        println!("\n  Database deletion cancelled. Other selected items will still be removed.\n");
        choices.database = false;
    }

    let mut summary = UninstallSummary::default();

    if choices.tool_configs {
        summary.tool_configs = Some(remove_tool_configs());
    }
    if choices.models {
        summary.models = Some(remove_directory(&paths.model_root));
    }
    if choices.database {
        summary.database = Some(remove_file(&paths.database_path));
        // WAL/SHM sidecars left by SQLite
        for ext in &["db-wal", "db-shm"] {
            let p = paths.database_path.with_extension(ext);
            if let Err(e) = std::fs::remove_file(&p)
                && e.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(path = %p.display(), err = %e, "failed to remove sidecar");
            }
        }
    }
    if choices.models || choices.database {
        summary.benchmarks = Some(remove_directory(&paths.benchmark_root));
    }

    let data_root_removed = if choices.models || choices.database {
        try_remove_empty_dir(&paths.data_root)
    } else {
        false
    };

    println!("\n  Uninstall summary:\n");

    if let Some(ref tc) = summary.tool_configs {
        match tc {
            ToolConfigResult::Removed { count, errors } => {
                if *count > 0 {
                    println!(
                        "    \u{2713} Tool configurations \u{2014} removed from {count} tool{}",
                        if *count == 1 { "" } else { "s" }
                    );
                }
                for (name, err) in errors {
                    println!("    \u{2717} {name} \u{2014} error: {err}");
                }
            }
            ToolConfigResult::NoneFound => {
                println!("    - Tool configurations \u{2014} none detected");
            }
        }
    }

    if let Some(ref r) = summary.models {
        print_remove_outcome("Models directory", r);
    }
    if let Some(ref r) = summary.database {
        print_remove_outcome("Database", r);
    }
    if let Some(ref r) = summary.benchmarks {
        print_remove_outcome("Benchmarks cache", r);
    }

    println!();
    if data_root_removed {
        println!(
            "  MAG data directory {} has been removed.",
            paths.data_root.display()
        );
    }
    println!("  To reinstall, run: cargo install mag-memory\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

struct UninstallChoices {
    tool_configs: bool,
    models: bool,
    database: bool,
}

#[derive(Default)]
struct UninstallSummary {
    tool_configs: Option<ToolConfigResult>,
    models: Option<RemoveOutcome>,
    database: Option<RemoveOutcome>,
    benchmarks: Option<RemoveOutcome>,
}

enum ToolConfigResult {
    Removed {
        count: usize,
        errors: Vec<(String, String)>,
    },
    NoneFound,
}

#[derive(Debug)]
enum RemoveOutcome {
    Removed { size: u64 },
    NotFound,
    Error(String),
}

fn print_remove_outcome(label: &str, outcome: &RemoveOutcome) {
    match outcome {
        RemoveOutcome::Removed { size } => {
            println!(
                "    \u{2713} {label} \u{2014} removed ({} freed)",
                format_size(*size)
            );
        }
        RemoveOutcome::NotFound => {
            println!("    - {label} \u{2014} not found");
        }
        RemoveOutcome::Error(e) => {
            println!("    \u{2717} {label} \u{2014} error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

fn read_line() -> Result<String> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading user input")?;
    Ok(line)
}

fn prompt_choices(paths: &app_paths::AppPaths) -> Result<UninstallChoices> {
    println!("\n  MAG Uninstall\n");
    println!("  What would you like to remove?\n");

    println!("    [1] Tool configurations (Claude Code, Cursor, VS Code, ...)");

    let models_label = path_size_label(&paths.model_root);
    println!("    [2] Downloaded models (~/.mag/models/, {models_label})");

    let db_label = path_size_label(&paths.database_path);
    println!("    [3] Database and all memories (~/.mag/memory.db, {db_label})");
    println!(
        "        \u{26a0}  This permanently deletes all stored memories, sessions, and relationships."
    );

    println!();
    println!("    [A] All of the above");
    println!();

    print!("  Enter choices (comma-separated, e.g. 1,2 or A): ");
    io::stdout().flush().context("flushing stdout")?;

    let trimmed = read_line()?.trim().to_lowercase();

    if trimmed.is_empty() {
        println!("  No choices selected. Nothing to do.");
        return Ok(UninstallChoices {
            tool_configs: false,
            models: false,
            database: false,
        });
    }

    if trimmed == "a" || trimmed == "all" {
        return Ok(UninstallChoices {
            tool_configs: true,
            models: true,
            database: true,
        });
    }

    let mut choices = UninstallChoices {
        tool_configs: false,
        models: false,
        database: false,
    };

    for part in trimmed.split(',') {
        match part.trim() {
            "1" => choices.tool_configs = true,
            "2" => choices.models = true,
            "3" => choices.database = true,
            other => {
                println!("  Unknown choice: {other}");
            }
        }
    }

    Ok(choices)
}

fn confirm_database_deletion() -> Result<bool> {
    println!();
    println!("  \u{26a0}  WARNING: This will permanently delete your MAG database containing all");
    println!("  memories, sessions, and relationships. This cannot be undone.");
    println!();
    print!("  Type \"delete my memories\" to confirm: ");
    io::stdout().flush().context("flushing stdout")?;

    Ok(read_line()?.trim() == "delete my memories")
}

// ---------------------------------------------------------------------------
// Removal helpers
// ---------------------------------------------------------------------------

fn remove_tool_configs() -> ToolConfigResult {
    let result = tool_detection::detect_all_tools(None);

    if result.detected.is_empty() {
        return ToolConfigResult::NoneFound;
    }

    let mut removed_count: usize = 0;
    let mut errors: Vec<(String, String)> = Vec::new();
    for dt in &result.detected {
        let name = dt.tool.display_name().to_string();

        // For Claude Code, also try to remove the plugin.
        if dt.tool == tool_detection::AiTool::ClaudeCode {
            match config_writer::remove_claude_plugin() {
                Ok(RemoveResult::Removed) => {
                    tracing::debug!("claude plugin removed");
                }
                Ok(_) => {
                    tracing::debug!("claude plugin was not installed");
                }
                Err(e) => {
                    tracing::debug!(error = %e, "claude plugin removal failed (claude CLI may not be installed)");
                }
            }
        }

        match config_writer::remove_config(dt) {
            Ok(RemoveResult::Removed) => {
                tracing::debug!(tool = %name, "removed config");
                removed_count += 1;
            }
            Ok(RemoveResult::UnsupportedFormat { reason }) => {
                tracing::warn!(tool = %name, reason = %reason, "unsupported config format");
                errors.push((name, format!("unsupported format: {reason}")));
            }
            Ok(_) => {
                tracing::debug!(tool = %name, "config not present");
            }
            Err(e) => {
                tracing::warn!(tool = %name, err = %e, "failed to remove config");
                errors.push((name, format!("{e:#}")));
            }
        }
    }

    if removed_count == 0 && errors.is_empty() {
        ToolConfigResult::NoneFound
    } else {
        ToolConfigResult::Removed {
            count: removed_count,
            errors,
        }
    }
}

fn remove_directory(path: &Path) -> RemoveOutcome {
    let size = dir_size(path);
    match std::fs::remove_dir_all(path) {
        Ok(()) => RemoveOutcome::Removed { size },
        Err(e) if e.kind() == io::ErrorKind::NotFound => RemoveOutcome::NotFound,
        Err(e) => RemoveOutcome::Error(format!("{e:#}")),
    }
}

fn remove_file(path: &Path) -> RemoveOutcome {
    let size = path.metadata().map(|m| m.len()).unwrap_or(0);
    match std::fs::remove_file(path) {
        Ok(()) => RemoveOutcome::Removed { size },
        Err(e) if e.kind() == io::ErrorKind::NotFound => RemoveOutcome::NotFound,
        Err(e) => RemoveOutcome::Error(format!("{e:#}")),
    }
}

/// Attempts to remove a directory only if it is empty (or contains only empty
/// subdirectories). Returns `true` if the directory was removed.
fn try_remove_empty_dir(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    if is_dir_effectively_empty(path) {
        std::fs::remove_dir_all(path).is_ok()
    } else {
        false
    }
}

/// Returns `true` if a directory is empty or contains only empty subdirectories.
/// Treats unreadable entries as non-empty (safe default — won't delete what we can't inspect).
fn is_dir_effectively_empty(path: &Path) -> bool {
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => return false,
        };
        if ft.is_file() || ft.is_symlink() {
            return false;
        }
        if ft.is_dir() && !is_dir_effectively_empty(&entry.path()) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Size helpers
// ---------------------------------------------------------------------------

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        } else if ft.is_dir() {
            total += dir_size(&entry.path());
        }
    }
    total
}

#[allow(clippy::cast_precision_loss)] // display-only formatting; precision loss is negligible
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

fn path_size_label(path: &Path) -> String {
    if !path.exists() {
        return "(not found)".to_string();
    }
    if path.is_dir() {
        format_size(dir_size(path))
    } else {
        format_size(path.metadata().map(|m| m.len()).unwrap_or(0))
    }
}

fn is_non_interactive() -> bool {
    !io::stdin().is_terminal()
        || std::env::var_os("CI").is_some()
        || std::env::var_os("GITHUB_ACTIONS").is_some()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    // -----------------------------------------------------------------------
    // format_size
    // -----------------------------------------------------------------------

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(340 * 1024), "340 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(2_200_000), "2.1 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    // -----------------------------------------------------------------------
    // dir_size
    // -----------------------------------------------------------------------

    #[test]
    fn dir_size_empty() {
        let dir = std::env::temp_dir().join(format!("mag-dirsize-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(dir_size(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_with_files() {
        let dir = std::env::temp_dir().join(format!("mag-dirsize-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(dir.join("sub/b.txt"), "world!").unwrap(); // 6 bytes
        assert_eq!(dir_size(&dir), 11);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_nonexistent() {
        let dir = std::env::temp_dir().join("mag-dirsize-nonexistent");
        assert_eq!(dir_size(&dir), 0);
    }

    // -----------------------------------------------------------------------
    // path_size_label
    // -----------------------------------------------------------------------

    #[test]
    fn path_size_label_not_found() {
        let p = std::env::temp_dir().join("mag-label-nonexistent");
        assert_eq!(path_size_label(&p), "(not found)");
    }

    // -----------------------------------------------------------------------
    // remove_directory / remove_file
    // -----------------------------------------------------------------------

    #[test]
    fn remove_directory_not_found() {
        let p = std::env::temp_dir().join("mag-rm-nonexistent");
        assert!(matches!(remove_directory(&p), RemoveOutcome::NotFound));
    }

    #[test]
    fn remove_directory_success() {
        let dir = std::env::temp_dir().join(format!("mag-rmdir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), "data").unwrap();
        match remove_directory(&dir) {
            RemoveOutcome::Removed { size } => assert_eq!(size, 4),
            other => panic!("Expected Removed, got {other:?}"),
        }
        assert!(!dir.exists());
    }

    #[test]
    fn remove_file_not_found() {
        let p = std::env::temp_dir().join("mag-rmfile-nonexistent");
        assert!(matches!(remove_file(&p), RemoveOutcome::NotFound));
    }

    #[test]
    fn remove_file_success() {
        let dir = std::env::temp_dir().join(format!("mag-rmfile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("test.db");
        std::fs::write(&f, "database").unwrap();
        match remove_file(&f) {
            RemoveOutcome::Removed { size } => assert_eq!(size, 8),
            other => panic!("Expected Removed, got {other:?}"),
        }
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // remove_tool_configs
    // -----------------------------------------------------------------------

    #[test]
    fn remove_tool_configs_with_configured_tool() {
        with_temp_home(|home| {
            // Create a Claude Code config with MAG
            let config_path = home.join(".claude.json");
            std::fs::write(
                &config_path,
                r#"{"mcpServers":{"mag":{"command":"mag","args":["serve"]},"other":{}}}"#,
            )
            .unwrap();

            let result = remove_tool_configs();
            match result {
                ToolConfigResult::Removed { count, errors } => {
                    assert!(count >= 1, "expected at least 1 removal");
                    assert!(errors.is_empty(), "expected no errors: {errors:?}");
                }
                ToolConfigResult::NoneFound => panic!("expected tool config removal"),
            }

            // Verify MAG was removed but other config preserved
            let content = std::fs::read_to_string(&config_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(parsed["mcpServers"]["mag"].is_null());
            assert!(parsed["mcpServers"]["other"].is_object());
        });
    }

    #[test]
    fn remove_tool_configs_no_tools() {
        with_temp_home(|_home| {
            let result = remove_tool_configs();
            assert!(matches!(result, ToolConfigResult::NoneFound));
        });
    }

    // -----------------------------------------------------------------------
    // try_remove_empty_dir
    // -----------------------------------------------------------------------

    #[test]
    fn try_remove_empty_dir_removes_empty() {
        let dir = std::env::temp_dir().join(format!("mag-tryrmdir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(try_remove_empty_dir(&dir));
        assert!(!dir.exists());
    }

    #[test]
    fn try_remove_empty_dir_nonexistent() {
        let dir = std::env::temp_dir().join("mag-tryrmdir-nonexistent");
        assert!(!try_remove_empty_dir(&dir));
    }

    // -----------------------------------------------------------------------
    // Integration: full uninstall with temp home
    // -----------------------------------------------------------------------

    #[test]
    fn integration_configs_only_preserves_data() {
        with_temp_home(|home| {
            // Create fake data
            let mag_dir = home.join(".mag");
            std::fs::create_dir_all(mag_dir.join("models")).unwrap();
            std::fs::write(mag_dir.join("models/model.onnx"), "model data").unwrap();
            std::fs::write(mag_dir.join("memory.db"), "database").unwrap();

            // Create a Claude Code config with MAG
            let config_path = home.join(".claude.json");
            std::fs::write(
                &config_path,
                r#"{"mcpServers":{"mag":{"command":"mag","args":["serve"]}}}"#,
            )
            .unwrap();

            // Run configs_only
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(run_uninstall(false, true)).unwrap();

            // Data should still exist
            assert!(mag_dir.join("models/model.onnx").exists());
            assert!(mag_dir.join("memory.db").exists());

            // MAG config should be removed
            let content = std::fs::read_to_string(&config_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(parsed["mcpServers"]["mag"].is_null());
        });
    }
}
