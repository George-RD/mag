//! Comprehensive uninstall command for MAG.
//!
//! Removes tool configurations, downloaded models, the database, and
//! the `~/.mag` data directory. Supports interactive, `--all`, and
//! `--configs-only` modes.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

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
            binary: false,
            models: false,
            database: false,
        }
    } else if all || is_non_interactive() {
        UninstallChoices {
            tool_configs: true,
            binary: all,
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
    if choices.binary {
        summary.binary = Some(remove_binary_and_path(&paths.home_dir));
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
            ToolConfigResult::Removed {
                count,
                errors,
                connector_count,
                connector_errors,
            } => {
                if *count > 0 {
                    println!(
                        "    \u{2713} Tool configurations \u{2014} removed from {count} tool{}",
                        if *count == 1 { "" } else { "s" }
                    );
                }
                for (name, err) in errors {
                    println!("    \u{2717} {name} \u{2014} error: {err}");
                }
                if *connector_count > 0 {
                    println!(
                        "    \u{2713} Connector content \u{2014} removed {connector_count} item{}",
                        if *connector_count == 1 { "" } else { "s" }
                    );
                }
                for (name, err) in connector_errors {
                    println!("    \u{2717} Connector {name} \u{2014} error: {err}");
                }
            }
            ToolConfigResult::NoneFound => {
                println!("    - Tool configurations \u{2014} none detected");
            }
        }
    }

    if let Some(ref br) = summary.binary {
        print_remove_outcome("MAG binary", &br.removed);
        for p in &br.profiles_cleaned {
            println!("    \u{2713} Cleaned PATH entry from {p}");
        }
        for (profile, reason) in &br.profiles_failed {
            println!("    \u{2717} {profile}: {reason}");
        }
        for hint in &br.install_hints {
            println!("    \u{2139} {hint}");
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
    if summary.binary.as_ref().is_some_and(|br| {
        matches!(br.removed, RemoveOutcome::Removed { .. }) || !br.profiles_cleaned.is_empty()
    }) {
        println!("  Restart your shell for PATH changes to take effect.");
    }
    println!("  To reinstall, run: cargo install mag-memory\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

struct UninstallChoices {
    tool_configs: bool,
    binary: bool,
    models: bool,
    database: bool,
}

#[derive(Default)]
struct UninstallSummary {
    tool_configs: Option<ToolConfigResult>,
    binary: Option<BinaryResult>,
    models: Option<RemoveOutcome>,
    database: Option<RemoveOutcome>,
    benchmarks: Option<RemoveOutcome>,
}

struct BinaryResult {
    removed: RemoveOutcome,
    profiles_cleaned: Vec<String>,
    profiles_failed: Vec<(String, String)>,
    /// Package-manager hints to show after removal (e.g. "run: cargo uninstall …").
    install_hints: Vec<String>,
}

enum ToolConfigResult {
    Removed {
        count: usize,
        errors: Vec<(String, String)>,
        connector_count: usize,
        connector_errors: Vec<(String, String)>,
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

    let bin_label = binary_install_label(&paths.home_dir);
    println!("    [2] Binary and PATH entries ({bin_label})");

    let models_label = path_size_label(&paths.model_root);
    println!("    [3] Downloaded models (~/.mag/models/, {models_label})");

    let db_label = path_size_label(&paths.database_path);
    println!("    [4] Database and all memories (~/.mag/memory.db, {db_label})");
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
            binary: false,
            models: false,
            database: false,
        });
    }

    if trimmed == "a" || trimmed == "all" {
        return Ok(UninstallChoices {
            tool_configs: true,
            binary: true,
            models: true,
            database: true,
        });
    }

    let mut choices = UninstallChoices {
        tool_configs: false,
        binary: false,
        models: false,
        database: false,
    };

    for part in trimmed.split(',') {
        match part.trim() {
            "1" => choices.tool_configs = true,
            "2" => choices.binary = true,
            "3" => choices.models = true,
            "4" => choices.database = true,
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

    // Always attempt to remove connector content (AGENTS.md sections,
    // OpenCode skills) regardless of whether any tool configs were detected.
    // The files may exist even if configs were manually removed.
    let (connector_count, connector_errors) = remove_connector_content();

    if result.detected.is_empty() {
        // If connector content was cleaned up even though no tool configs were
        // found, surface that so the user sees something was done.
        if connector_count > 0 || !connector_errors.is_empty() {
            return ToolConfigResult::Removed {
                count: 0,
                errors: vec![],
                connector_count,
                connector_errors,
            };
        }
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
                    tracing::warn!(error = %e, "claude plugin removal failed");
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

    if removed_count == 0
        && errors.is_empty()
        && connector_count == 0
        && connector_errors.is_empty()
    {
        ToolConfigResult::NoneFound
    } else {
        ToolConfigResult::Removed {
            count: removed_count,
            errors,
            connector_count,
            connector_errors,
        }
    }
}

/// Removes connector content installed by `mag setup`: MAG sections in
/// AGENTS.md files and OpenCode skill directories.
///
/// Returns `(count, errors)` where `count` is the number of items removed and
/// `errors` is a list of `(label, error_message)` pairs for any failures.
fn remove_connector_content() -> (usize, Vec<(String, String)>) {
    let home = match app_paths::home_dir() {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "cannot resolve HOME for connector content removal");
            return (0, vec![("home".to_string(), format!("{e:#}"))]);
        }
    };

    let mut count = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();

    // Use agents_md_target to derive paths rather than hardcoding them.
    for &tool in &[
        tool_detection::AiTool::Codex,
        tool_detection::AiTool::GeminiCli,
    ] {
        if let Some((_, path)) = crate::setup::agents_md_target(tool, &home) {
            let label = format!("{} AGENTS.md", tool.display_name());
            match crate::setup::remove_agents_md_section(&path) {
                Ok(true) => {
                    tracing::info!(path = %path.display(), "removed MAG section");
                    count += 1;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to clean AGENTS.md");
                    errors.push((label, format!("{e:#}")));
                }
            }
        }
    }

    match crate::setup::remove_opencode_skills(&home) {
        Ok(n) if n > 0 => {
            tracing::info!(count = n, "removed OpenCode skill directories");
            count += n;
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, "failed to remove OpenCode skills");
            errors.push(("OpenCode skills".to_string(), format!("{e:#}")));
        }
    }

    (count, errors)
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
        // Use recursive remove_dir (not remove_dir_all) so that if a concurrent
        // process creates files after the emptiness check, remove_dir fails safely
        // with "directory not empty" instead of deleting the new content.
        match remove_empty_tree(path) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to remove directory");
                false
            }
        }
    } else {
        false
    }
}

/// Recursively removes a directory tree that should only contain empty subdirectories.
/// Uses `remove_dir` (not `remove_dir_all`) at each level so that if a concurrent
/// process creates files after the emptiness check, the operation fails safely.
fn remove_empty_tree(path: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_empty_tree(&entry.path())?;
        }
    }
    std::fs::remove_dir(path)
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
// Binary & PATH removal
// ---------------------------------------------------------------------------

/// Returns the default MAG binary install directory, respecting `MAG_INSTALL_DIR`.
fn default_install_dir(home: &Path) -> PathBuf {
    std::env::var_os("MAG_INSTALL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".mag").join("bin"))
}

/// Returns the path of the currently running `mag` binary.
///
/// On Linux, `/proc/self/exe` may be suffixed with ` (deleted)` if the
/// binary was replaced after startup — that suffix is stripped so path
/// matching and display work correctly.
///
/// The path is intentionally **not** canonicalized: canonicalization follows
/// symlinks, which would resolve Homebrew keg paths, Nix store paths, or
/// stow-managed symlinks to locations that don't match the install method.
fn current_exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|p| {
        let s = p.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(" (deleted)") {
            PathBuf::from(stripped)
        } else {
            p
        }
    })
}

/// Returns a hint string telling the user which package manager command to run
/// for full metadata cleanup, inferred from the binary path.
///
/// This is advisory only — the binary is always removed directly regardless of
/// install method.  The path is not canonicalized, so the patterns match the
/// path the user put the binary at, not a resolved symlink target.
fn install_method_hint(exe: &Path) -> Option<&'static str> {
    let s = exe.to_string_lossy();
    if s.contains("/.cargo/bin/") {
        Some("Installed via Cargo — run: cargo uninstall mag-memory")
    } else if s.contains("/homebrew/") || s.contains("/Cellar/") || s.contains("/linuxbrew/") {
        Some("Installed via Homebrew — run: brew uninstall mag  (to clean up Homebrew metadata)")
    } else if s.contains("/.local/share/uv/tools/") || s.contains("/.uv/tools/") {
        Some("Installed via uv — run: uv tool uninstall mag-memory")
    } else if s.contains("/site-packages/") || s.contains("/dist-packages/") {
        Some("Installed via pip — run: pip uninstall -y mag-memory")
    } else if s.contains("/node_modules/") {
        Some("Installed via npm — run: npm uninstall -g mag-memory")
    } else {
        None
    }
}

/// Label for the binary install location shown in the interactive menu.
fn binary_install_label(home: &Path) -> String {
    if let Some(exe) = current_exe_path() {
        return format!("{}", exe.display());
    }
    // Fallback when current_exe() is unavailable (should be rare).
    default_install_dir(home).join("mag").display().to_string()
}

/// Removes the MAG binary and cleans PATH entries from shell profiles.
fn remove_binary_and_path(home: &Path) -> BinaryResult {
    let exe = if let Some(p) = current_exe_path() {
        p
    } else {
        let fallback = default_install_dir(home).join("mag");
        tracing::warn!(
            fallback = %fallback.display(),
            "current_exe() unavailable; falling back to default install directory"
        );
        fallback
    };
    remove_binary_and_path_impl(home, exe)
}

/// Inner implementation; accepts the exe path explicitly so tests can inject a
/// controlled path without depending on `current_exe()`.
fn remove_binary_and_path_impl(home: &Path, binary_path: PathBuf) -> BinaryResult {
    let install_dir = default_install_dir(home);

    // Safety guard: refuse to delete anything that isn't named "mag" (or
    // "mag.exe" on Windows) to prevent accidental deletion of an unrelated
    // binary if current_exe() or a caller supplies an unexpected path.
    let expected_name = if cfg!(windows) { "mag.exe" } else { "mag" };
    let actual_name = binary_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if actual_name != expected_name {
        tracing::warn!(
            path = %binary_path.display(),
            expected = expected_name,
            actual = actual_name,
            "refusing to delete binary: filename does not match expected 'mag'"
        );
        return BinaryResult {
            removed: RemoveOutcome::NotFound,
            profiles_cleaned: vec![],
            profiles_failed: vec![],
            install_hints: vec![],
        };
    }

    // Build package-manager hints.
    let mut install_hints: Vec<String> = Vec::new();
    if let Some(hint) = install_method_hint(&binary_path) {
        install_hints.push(hint.to_owned());
    }
    // If the running binary is NOT in ~/.cargo/bin but a cargo binary exists
    // there as well (e.g. shell-installer + cargo both present), surface that.
    let cargo_bin = home.join(".cargo").join("bin").join("mag");
    let running_from_cargo = binary_path.to_string_lossy().contains("/.cargo/bin/");
    if !running_from_cargo && cargo_bin.symlink_metadata().is_ok() {
        install_hints.push("Also installed via Cargo — run: cargo uninstall mag-memory".to_owned());
    }

    // Remove the binary.
    let removed = if binary_path.exists() {
        let outcome = remove_file(&binary_path);
        if matches!(outcome, RemoveOutcome::Removed { .. }) {
            // Clean up the parent dir only if it's the MAG install dir.
            if binary_path.parent() == Some(install_dir.as_path()) {
                try_remove_empty_dir(&install_dir);
            }
        }
        outcome
    } else {
        RemoveOutcome::NotFound
    };

    // Clean PATH from shell profiles (always attempt, regardless of install method).
    let install_dir_str = install_dir.to_string_lossy().to_string();
    let mut profiles_cleaned = Vec::new();
    let mut profiles_failed: Vec<(String, String)> = Vec::new();

    for profile_path in shell_profiles(home) {
        if !profile_path.exists() {
            continue;
        }
        match clean_path_from_profile(&profile_path, &install_dir_str) {
            Ok(true) => {
                tracing::info!(profile = %profile_path.display(), "cleaned PATH entry");
                profiles_cleaned.push(profile_path.display().to_string());
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    profile = %profile_path.display(),
                    error = %e,
                    "failed to clean PATH from profile"
                );
                profiles_failed.push((profile_path.display().to_string(), format!("{e:#}")));
            }
        }
    }

    // Also clean the binary's actual parent dir from shell profiles if it
    // differs from the default install dir (e.g. ~/.cargo/bin, Homebrew prefix).
    if let Some(binary_dir) = binary_path.parent()
        && binary_dir != install_dir.as_path()
    {
        let binary_dir_str = binary_dir.to_string_lossy().to_string();
        for profile_path in shell_profiles(home) {
            if !profile_path.exists() {
                continue;
            }
            match clean_path_from_profile(&profile_path, &binary_dir_str) {
                Ok(true) => {
                    tracing::info!(
                        profile = %profile_path.display(),
                        dir = %binary_dir_str,
                        "cleaned alternate PATH entry"
                    );
                    let display = profile_path.display().to_string();
                    if !profiles_cleaned.contains(&display) {
                        profiles_cleaned.push(display);
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        profile = %profile_path.display(),
                        error = %e,
                        "failed to clean alternate PATH from profile"
                    );
                    let display = profile_path.display().to_string();
                    if !profiles_failed.iter().any(|(p, _)| p == &display) {
                        profiles_failed.push((display, format!("{e:#}")));
                    }
                }
            }
        }
    }

    BinaryResult {
        removed,
        profiles_cleaned,
        profiles_failed,
        install_hints,
    }
}

/// Returns paths to common shell profile files.
fn shell_profiles(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".zshrc"),
        home.join(".bash_profile"),
        home.join(".bashrc"),
        app_paths::xdg_config_home(home)
            .join("fish")
            .join("config.fish"),
    ]
}

/// Removes the `# MAG` marker block written by the installer from a shell
/// profile. The installer appends:
///
/// ```text
/// (blank line)
/// # MAG
/// export PATH="$HOME/.mag/bin:$PATH"   # or fish_add_path equivalent
/// ```
///
/// This function removes the `# MAG` line and the PATH line that immediately
/// follows it. If no `# MAG` marker is found the file is left unchanged.
/// Returns `Ok(true)` if the file was modified.
fn clean_path_from_profile(profile: &Path, _install_dir: &str) -> Result<bool> {
    let content = std::fs::read_to_string(profile)
        .with_context(|| format!("reading {}", profile.display()))?;

    if !content.lines().any(|l| l.trim() == "# MAG") {
        return Ok(false);
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "# MAG" {
            // Skip the `# MAG` marker line and the PATH line that follows it.
            i += 1; // skip marker
            if i < lines.len() {
                i += 1; // skip PATH line
            }
            // Also drop a preceding blank line that was written by the installer
            // as a separator, but only if the last line we kept is blank.
            if out
                .last()
                .map(|l: &&str| l.trim().is_empty())
                .unwrap_or(false)
            {
                out.pop();
            }
        } else {
            out.push(lines[i]);
            i += 1;
        }
    }

    let mut new_content = out.join("\n");
    // Preserve trailing newline if the original had one
    if content.ends_with('\n') && !new_content.is_empty() {
        new_content.push('\n');
    }

    if new_content == content {
        return Ok(false);
    }

    std::fs::write(profile, new_content)
        .with_context(|| format!("writing cleaned {}", profile.display()))?;

    Ok(true)
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
                ToolConfigResult::Removed { count, errors, .. } => {
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
    // clean_path_from_profile
    // -----------------------------------------------------------------------

    #[test]
    fn clean_path_removes_mag_lines() {
        let dir = std::env::temp_dir().join(format!("mag-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let profile = dir.join(".zshrc");
        std::fs::write(
            &profile,
            "export PATH=\"/usr/bin:$PATH\"\n\n# MAG\nexport PATH=\"/home/user/.mag/bin:$PATH\"\n",
        )
        .unwrap();

        assert!(clean_path_from_profile(&profile, "/home/user/.mag/bin").unwrap());

        let content = std::fs::read_to_string(&profile).unwrap();
        assert!(!content.contains("# MAG"));
        assert!(!content.contains(".mag/bin"));
        assert!(content.contains("/usr/bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_path_no_match_returns_false() {
        let dir = std::env::temp_dir().join(format!("mag-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let profile = dir.join(".zshrc");
        std::fs::write(&profile, "export PATH=\"/usr/bin:$PATH\"\n").unwrap();

        assert!(!clean_path_from_profile(&profile, "/home/user/.mag/bin").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Integration: full uninstall with temp home
    // -----------------------------------------------------------------------

    #[test]
    fn remove_binary_and_path_removes_binary() {
        // Unset MAG_INSTALL_DIR so default_install_dir uses the test home dir.
        let saved_install_dir = std::env::var_os("MAG_INSTALL_DIR");
        // SAFETY: single-threaded test; no other thread reads MAG_INSTALL_DIR concurrently.
        unsafe { std::env::remove_var("MAG_INSTALL_DIR") };

        let home = std::env::temp_dir().join(format!("mag-binrm-{}", uuid::Uuid::new_v4()));
        let bin_dir = home.join(".mag").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("mag"), "binary").unwrap();

        // Create a shell profile with PATH entry
        std::fs::write(
            home.join(".zshrc"),
            format!(
                "# existing\n\n# MAG\nexport PATH=\"{}:$PATH\"\n",
                bin_dir.display()
            ),
        )
        .unwrap();

        // Use _impl to inject a controlled binary path rather than current_exe().
        let result = remove_binary_and_path_impl(&home, bin_dir.join("mag"));
        assert!(matches!(result.removed, RemoveOutcome::Removed { .. }));
        assert!(!bin_dir.join("mag").exists());
        assert_eq!(result.profiles_cleaned.len(), 1);
        assert!(result.install_hints.is_empty());

        let profile = std::fs::read_to_string(home.join(".zshrc")).unwrap();
        assert!(!profile.contains(".mag/bin"));
        let _ = std::fs::remove_dir_all(&home);

        // Restore MAG_INSTALL_DIR if it was set before the test.
        // SAFETY: single-threaded test; no other thread reads MAG_INSTALL_DIR concurrently.
        if let Some(val) = saved_install_dir {
            unsafe { std::env::set_var("MAG_INSTALL_DIR", val) };
        }
    }

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

    // -----------------------------------------------------------------------
    // current_exe_path — suffix-stripping logic
    // -----------------------------------------------------------------------

    /// Mirrors the stripping logic inside `current_exe_path()` so it can be
    /// tested without calling `std::env::current_exe()`.
    fn strip_deleted_suffix(p: PathBuf) -> PathBuf {
        let s = p.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(" (deleted)") {
            PathBuf::from(stripped)
        } else {
            p
        }
    }

    #[test]
    fn current_exe_path_strips_deleted_suffix() {
        let input = PathBuf::from("/home/user/.mag/bin/mag (deleted)");
        assert_eq!(
            strip_deleted_suffix(input),
            PathBuf::from("/home/user/.mag/bin/mag")
        );
    }

    #[test]
    fn current_exe_path_unchanged_without_suffix() {
        let input = PathBuf::from("/home/user/.mag/bin/mag");
        assert_eq!(strip_deleted_suffix(input.clone()), input);
    }

    // -----------------------------------------------------------------------
    // install_method_hint
    // -----------------------------------------------------------------------

    #[test]
    fn install_method_hint_cargo() {
        let p = PathBuf::from("/home/user/.cargo/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some("Installed via Cargo — run: cargo uninstall mag-memory")
        );
    }

    #[test]
    fn install_method_hint_homebrew_cellar() {
        let p = PathBuf::from("/usr/local/Cellar/mag/1.0.0/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some(
                "Installed via Homebrew — run: brew uninstall mag  (to clean up Homebrew metadata)"
            )
        );
    }

    #[test]
    fn install_method_hint_homebrew_prefix() {
        let p = PathBuf::from("/opt/homebrew/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some(
                "Installed via Homebrew — run: brew uninstall mag  (to clean up Homebrew metadata)"
            )
        );
    }

    #[test]
    fn install_method_hint_uv_local_share() {
        let p = PathBuf::from("/home/user/.local/share/uv/tools/mag-memory/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some("Installed via uv — run: uv tool uninstall mag-memory")
        );
    }

    #[test]
    fn install_method_hint_uv_dot() {
        let p = PathBuf::from("/home/user/.uv/tools/mag-memory/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some("Installed via uv — run: uv tool uninstall mag-memory")
        );
    }

    #[test]
    fn install_method_hint_pip_site_packages() {
        let p = PathBuf::from("/usr/lib/python3/site-packages/mag_memory/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some("Installed via pip — run: pip uninstall -y mag-memory")
        );
    }

    #[test]
    fn install_method_hint_npm_node_modules() {
        let p =
            PathBuf::from("/home/user/.nvm/versions/node/v20/lib/node_modules/mag-memory/bin/mag");
        assert_eq!(
            install_method_hint(&p),
            Some("Installed via npm — run: npm uninstall -g mag-memory")
        );
    }

    #[test]
    fn install_method_hint_none_for_default_install() {
        let p = PathBuf::from("/home/user/.mag/bin/mag");
        assert_eq!(install_method_hint(&p), None);
    }

    // -----------------------------------------------------------------------
    // remove_binary_and_path_impl — extended cases
    // -----------------------------------------------------------------------

    #[test]
    fn remove_binary_impl_cargo_path_produces_hint() {
        let home = std::env::temp_dir().join(format!("mag-hint-cargo-{}", uuid::Uuid::new_v4()));
        let cargo_bin = home.join(".cargo").join("bin");
        std::fs::create_dir_all(&cargo_bin).unwrap();
        std::fs::write(cargo_bin.join("mag"), "binary").unwrap();

        let result = remove_binary_and_path_impl(&home, cargo_bin.join("mag"));
        assert!(matches!(result.removed, RemoveOutcome::Removed { .. }));
        assert!(
            result
                .install_hints
                .iter()
                .any(|h| h.contains("Installed via Cargo")),
            "expected Cargo hint, got: {:?}",
            result.install_hints
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn remove_binary_impl_not_found_returns_not_found() {
        let home = std::env::temp_dir().join(format!("mag-notfound-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let missing = home.join(".mag").join("bin").join("mag");
        let result = remove_binary_and_path_impl(&home, missing);
        assert!(matches!(result.removed, RemoveOutcome::NotFound));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn remove_binary_impl_dual_install_secondary_hint() {
        let home = std::env::temp_dir().join(format!("mag-dual-{}", uuid::Uuid::new_v4()));
        let bin_dir = home.join(".mag").join("bin");
        let cargo_bin = home.join(".cargo").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&cargo_bin).unwrap();
        std::fs::write(bin_dir.join("mag"), "binary").unwrap();
        std::fs::write(cargo_bin.join("mag"), "cargo binary").unwrap();

        let result = remove_binary_and_path_impl(&home, bin_dir.join("mag"));
        assert!(matches!(result.removed, RemoveOutcome::Removed { .. }));
        assert!(
            result
                .install_hints
                .iter()
                .any(|h| h.contains("Also installed via Cargo")),
            "expected secondary Cargo hint, got: {:?}",
            result.install_hints
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn remove_binary_impl_wrong_filename_guard() {
        let home = std::env::temp_dir().join(format!("mag-guard-{}", uuid::Uuid::new_v4()));
        let bin_dir = home.join(".mag").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let wrong = bin_dir.join("not-mag");
        std::fs::write(&wrong, "should not be deleted").unwrap();

        let result = remove_binary_and_path_impl(&home, wrong.clone());
        assert!(
            matches!(result.removed, RemoveOutcome::NotFound),
            "expected NotFound from filename guard"
        );
        assert!(
            wrong.exists(),
            "guard must not delete a file with wrong name"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    // -----------------------------------------------------------------------
    // clean_path_from_profile — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn clean_path_mag_marker_as_last_line_no_panic() {
        let dir =
            std::env::temp_dir().join(format!("mag-profile-lastline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let profile = dir.join(".zshrc");
        // # MAG is the last line with no following PATH line
        std::fs::write(&profile, "export FOO=bar\n# MAG").unwrap();

        let result = clean_path_from_profile(&profile, "/home/user/.mag/bin");
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&profile).unwrap();
        assert!(!content.contains("# MAG"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_path_no_trailing_newline() {
        let dir =
            std::env::temp_dir().join(format!("mag-profile-nonewline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let profile = dir.join(".zshrc");
        std::fs::write(
            &profile,
            "export PATH=\"/usr/bin:$PATH\"\n\n# MAG\nexport PATH=\"/home/user/.mag/bin:$PATH\"",
        )
        .unwrap();

        let result = clean_path_from_profile(&profile, "/home/user/.mag/bin");
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&profile).unwrap();
        assert!(!content.contains("# MAG"));
        assert!(!content.contains(".mag/bin"));
        assert!(content.contains("/usr/bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
