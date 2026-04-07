//! Interactive setup wizard for configuring AI coding tools to use MAG.
//!
//! The `mag setup` subcommand detects installed AI tools, presents their
//! configuration status, and writes MCP config entries so that each tool
//! can communicate with the MAG daemon.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config_writer::{self, ConfigWriteResult, TransportMode};
use crate::tool_detection::{self, ContentTier, DetectedTool, DetectionResult, MagConfigStatus};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Arguments for the `mag setup` subcommand, mapped from the CLI layer.
pub struct SetupArgs {
    pub non_interactive: bool,
    pub tools: Option<Vec<String>>,
    pub transport: TransportMode,
    pub port: u16,
    pub no_start: bool,
    pub uninstall: bool,
    pub force: bool,
}

/// Summary of a configuration run.
#[derive(Debug, Default)]
struct ConfigureSummary {
    written: Vec<String>,
    already_current: Vec<String>,
    unsupported: Vec<(String, String)>,
    deferred: Vec<String>,
    errors: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Main entry point for `mag setup`.
pub async fn run_setup(args: SetupArgs) -> Result<()> {
    if args.uninstall {
        return crate::uninstall::run_uninstall(false, true).await;
    }

    // Detect phase
    println!("\n  Detecting AI coding tools...\n");
    let result: DetectionResult = tokio::task::spawn_blocking(|| detect_phase(None))
        .await
        .context("tool detection task panicked")?;

    present_detection(&result);

    // Determine which tools to configure
    let tools_to_configure = select_tools(&result, &args)?;

    if tools_to_configure.is_empty() {
        println!("  No tools to configure.");
        return Ok(());
    }

    let summary = configure_tools(&tools_to_configure, args.transport, &tools_to_configure)?;
    present_summary(&summary);

    // Daemon phase
    #[cfg(feature = "daemon-http")]
    maybe_start_daemon(args.port, args.no_start)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Detection phase
// ---------------------------------------------------------------------------

fn detect_phase(project_root: Option<&Path>) -> DetectionResult {
    tool_detection::detect_all_tools(project_root)
}

// ---------------------------------------------------------------------------
// Presentation
// ---------------------------------------------------------------------------

fn present_detection(result: &DetectionResult) {
    if result.detected.is_empty() {
        println!("  No AI coding tools detected.\n");
        return;
    }

    println!("  Detected tools:\n");
    for dt in &result.detected {
        let status_icon = match &dt.mag_status {
            MagConfigStatus::Configured => "\u{2713}", // check mark
            MagConfigStatus::InstalledAsPlugin => "\u{2713}", // check mark
            MagConfigStatus::NotConfigured => "\u{2717}", // X mark
            MagConfigStatus::Misconfigured(_) => "\u{26a0}", // warning
            MagConfigStatus::Unreadable(_) => "\u{26a0}", // warning
        };
        println!(
            "    {status_icon} {name:<20} {status_label}",
            name = dt.tool.display_name(),
            status_label = status_short_label(&dt.mag_status),
        );
        tracing::debug!(
            tool = %dt.tool.display_name(),
            path = %dt.config_path.display(),
            "detected tool"
        );
    }

    if !result.not_found.is_empty() {
        println!();
        let not_found_names: Vec<&str> = result
            .not_found
            .iter()
            .map(|t: &tool_detection::AiTool| t.display_name())
            .collect();
        tracing::debug!(tools = ?not_found_names, "tools not found");
    }
    println!();
}

fn present_summary(summary: &ConfigureSummary) {
    println!("  Configuration summary:\n");

    for name in &summary.written {
        println!("    \u{2713} {name} — configured");
    }
    for name in &summary.already_current {
        println!("    \u{2713} {name} — already current");
    }
    for name in &summary.deferred {
        println!("    - {name} — deferred (format not yet supported)");
    }
    for (name, reason) in &summary.unsupported {
        println!("    - {name} — skipped ({reason})");
    }
    for (name, err) in &summary.errors {
        println!("    \u{2717} {name} — error: {err}");
    }
    println!();
}

// ---------------------------------------------------------------------------
// Tool selection
// ---------------------------------------------------------------------------

/// Returns the invocation-scoped tool set: all detected tools that match the
/// `--tools` filter (or all detected tools when no filter is given).  This is
/// the set used as the connector-content target so that AGENTS.md / SKILL.md
/// is written only for tools the user explicitly asked about, while still
/// including already-configured tools within that scope.
fn invocation_scoped_tools<'a>(
    result: &'a DetectionResult,
    args: &SetupArgs,
) -> Vec<&'a DetectedTool> {
    if let Some(ref tool_names) = args.tools {
        let lower_names: Vec<String> = tool_names.iter().map(|n| n.to_lowercase()).collect();
        result
            .detected
            .iter()
            .filter(|dt| {
                let display_lower = dt.tool.display_name().to_lowercase();
                let variant_lower = format!("{:?}", dt.tool).to_lowercase();
                lower_names.iter().any(|n| {
                    display_lower.contains(n.as_str()) || variant_lower.contains(n.as_str())
                })
            })
            .collect()
    } else {
        result.detected.iter().collect()
    }
}

fn select_tools<'a>(
    result: &'a DetectionResult,
    args: &SetupArgs,
) -> Result<Vec<&'a DetectedTool>> {
    let candidates = invocation_scoped_tools(result, args);

    // In force mode, configure all matched tools regardless of status
    if args.force {
        return Ok(candidates);
    }

    // Filter to only unconfigured/misconfigured tools
    let actionable: Vec<&DetectedTool> = candidates
        .into_iter()
        .filter(|dt| {
            !matches!(
                dt.mag_status,
                MagConfigStatus::Configured | MagConfigStatus::InstalledAsPlugin
            )
        })
        .collect();

    if actionable.is_empty() {
        return Ok(vec![]);
    }

    // Non-interactive: configure all actionable tools
    if args.non_interactive || is_ci() || !is_tty() {
        return Ok(actionable);
    }

    // Interactive: prompt user
    select_tools_interactive(&actionable)
}

fn status_short_label(status: &MagConfigStatus) -> &str {
    match status {
        MagConfigStatus::Configured => "configured",
        MagConfigStatus::InstalledAsPlugin => "installed as plugin",
        MagConfigStatus::NotConfigured => "not configured",
        MagConfigStatus::Misconfigured(r) => r.as_str(),
        MagConfigStatus::Unreadable(r) => r.as_str(),
    }
}

fn select_tools_interactive<'a>(tools: &[&'a DetectedTool]) -> Result<Vec<&'a DetectedTool>> {
    if tools.len() == 1 {
        let tool = tools[0];
        print!(
            "  Configure {} ({})? [Y/n] ",
            tool.tool.display_name(),
            status_short_label(&tool.mag_status),
        );
        io::stdout().flush().context("flushing stdout")?;
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .context("reading user input")?;
        let trimmed = line.trim().to_lowercase();
        return if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
            Ok(tools.to_vec())
        } else {
            Ok(vec![])
        };
    }

    // Multiple tools: show numbered list and accept Y/n or a selection like "1,3"
    println!("  Tools to configure:");
    for (i, dt) in tools.iter().enumerate() {
        println!(
            "    {}. {:<20} ({})",
            i + 1,
            dt.tool.display_name(),
            status_short_label(&dt.mag_status),
        );
    }
    println!();
    print!("  Configure all {}? [Y/n or e.g. 1,3] ", tools.len());
    io::stdout().flush().context("flushing stdout")?;

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading user input")?;
    let trimmed = line.trim().to_lowercase();

    if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
        Ok(tools.to_vec())
    } else if trimmed == "n" || trimmed == "no" {
        Ok(vec![])
    } else {
        // Parse comma/space-separated numbers like "1,3" or "1 3"
        let selected: Vec<&DetectedTool> = trimmed
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<usize>().ok())
            .filter(|&n| n >= 1 && n <= tools.len())
            .map(|n| tools[n - 1])
            .collect();
        Ok(selected)
    }
}

// ---------------------------------------------------------------------------
// Configuration phase
// ---------------------------------------------------------------------------

fn configure_tools(
    tools: &[&DetectedTool],
    mode: TransportMode,
    all_detected: &[&DetectedTool],
) -> Result<ConfigureSummary> {
    let mut summary = ConfigureSummary::default();

    for tool in tools {
        let name = tool.tool.display_name().to_string();

        // For Claude Code, prefer the plugin marketplace install.
        // The plugin's .mcp.json uses sh -c to resolve the binary via
        // $MAG_INSTALL_DIR or $HOME/.mag/bin without needing mag on PATH.
        // Fall back to writing .claude.json only when the plugin install fails.
        if tool.tool == tool_detection::AiTool::ClaudeCode {
            match config_writer::install_claude_plugin() {
                Ok(ConfigWriteResult::Plugin) => {
                    summary.written.push(format!("{name} (plugin)"));
                    continue;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "plugin install failed, falling back to MCP config");
                    // Fall through to regular write_config below
                }
                Ok(other) => {
                    tracing::debug!(result = ?other, "unexpected plugin install result, falling back");
                    // Fall through
                }
            }
        }

        match config_writer::write_config(tool, mode) {
            Ok(ConfigWriteResult::Written { backup_path }) => {
                if let Some(ref bak) = backup_path {
                    tracing::debug!(tool = %name, backup = %bak.display(), "config backed up");
                }
                let _ = backup_path; // suppress unused warning
                summary.written.push(name);
            }
            Ok(ConfigWriteResult::AlreadyCurrent) => {
                summary.already_current.push(name);
            }
            Ok(ConfigWriteResult::UnsupportedFormat { reason }) => {
                summary.unsupported.push((name, reason));
            }
            Ok(ConfigWriteResult::Deferred { tool: ai_tool }) => {
                summary.deferred.push(ai_tool.display_name().to_string());
            }
            Ok(ConfigWriteResult::Plugin) => {
                // Shouldn't reach here for non-Claude tools, but handle it
                summary.written.push(format!("{name} (plugin)"));
            }
            Err(e) => {
                summary.errors.push((name, format!("{e:#}")));
            }
        }
    }

    // Phase 2: Install connector content (AGENTS.md, SKILL.md, etc.) for the
    // full invocation scope, not just the newly-configured subset.
    let (connector_successes, connector_warnings) = install_connector_content(all_detected);
    summary.written.extend(connector_successes);
    summary.errors.extend(connector_warnings);

    Ok(summary)
}

// ---------------------------------------------------------------------------
// Helpers: atomic write, XDG config
// ---------------------------------------------------------------------------

/// Atomically writes `content` to `path` by writing to a temporary file in the
/// same directory, flushing, syncing, then renaming. This prevents partial writes
/// if the process is interrupted.
///
/// The temp file includes the process ID in its name to avoid races when
/// multiple `mag setup` invocations run concurrently. If any step fails, the
/// temp file is removed before the error is returned.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("mag-tmp.{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("creating temp file {}", tmp.display()))?;
        f.write_all(content.as_bytes())
            .with_context(|| format!("writing to {}", tmp.display()))?;
        f.sync_all()
            .with_context(|| format!("syncing {}", tmp.display()))?;
        drop(f);
        std::fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------------
// Connector content installation
// ---------------------------------------------------------------------------

/// Sentinel marking the start of the MAG section in AGENTS.md files.
const MAG_SENTINEL_START: &str = "<!-- MAG_MEMORY_START -->";
/// Sentinel marking the end of the MAG section in AGENTS.md files.
const MAG_SENTINEL_END: &str = "<!-- MAG_MEMORY_END -->";

/// Shared AGENTS.md template for all AgentsMd-tier tools.
/// The `{{MAG_VERSION}}` placeholder is replaced at install time with the
/// running binary's version so stale installs can be detected after upgrades.
const AGENTS_MD_TEMPLATE: &str = include_str!("../connectors/shared/AGENTS.md");

fn agents_md_content() -> String {
    AGENTS_MD_TEMPLATE.replace("{{MAG_VERSION}}", env!("CARGO_PKG_VERSION"))
}

/// OpenCode skill definitions: (directory name, embedded content).
const OPENCODE_SKILLS: &[(&str, &str)] = &[
    (
        "memory-store",
        include_str!("../connectors/opencode/skills/memory-store/SKILL.md"),
    ),
    (
        "memory-recall",
        include_str!("../connectors/opencode/skills/memory-recall/SKILL.md"),
    ),
    (
        "memory-checkpoint",
        include_str!("../connectors/opencode/skills/memory-checkpoint/SKILL.md"),
    ),
    (
        "memory-health",
        include_str!("../connectors/opencode/skills/memory-health/SKILL.md"),
    ),
];

/// Installs connector content (AGENTS.md / SKILL.md) for each tool based on
/// its content tier. Returns `(successes, warnings)` — human-readable messages
/// for successful installs and `(tool_label, message)` pairs for any failures.
fn install_connector_content(tools: &[&DetectedTool]) -> (Vec<String>, Vec<(String, String)>) {
    let mut successes = Vec::new();
    let mut warnings: Vec<(String, String)> = Vec::new();

    let home = match crate::app_paths::home_dir() {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("Cannot resolve HOME for connector content: {e}");
            tracing::warn!("{msg}");
            warnings.push(("connector".to_string(), msg));
            return (successes, warnings);
        }
    };

    for tool in tools {
        let name = tool.tool.display_name();
        match tool.tool.content_tier() {
            ContentTier::AgentsMd => match install_agents_md(tool.tool, &home) {
                Ok(true) => {
                    let msg = format!("Installed AGENTS.md for {name}");
                    tracing::debug!(tool = %name, "AGENTS.md installed/updated");
                    successes.push(msg);
                }
                Ok(false) => {
                    tracing::debug!(tool = %name, "AGENTS.md already current");
                }
                Err(e) => {
                    let msg = format!("Failed to install AGENTS.md: {e}");
                    tracing::warn!(tool = %name, "{msg}");
                    warnings.push((name.to_string(), msg));
                }
            },
            ContentTier::Skills => match install_skills(tool.tool, &home) {
                Ok(n) if n > 0 => {
                    let msg = format!("Installed {n} skill(s) for {name}");
                    tracing::debug!(tool = %name, count = n, "installed SKILL.md files");
                    successes.push(msg);
                }
                Ok(_) => {
                    tracing::debug!(tool = %name, "skills already current");
                }
                Err(e) => {
                    let msg = format!("Failed to install SKILL.md files: {e}");
                    tracing::warn!(tool = %name, "{msg}");
                    warnings.push((name.to_string(), msg));
                }
            },
            ContentTier::Rules => tracing::debug!(tool = %name, "rules connector deferred"),
            ContentTier::Mcp | ContentTier::Plugin => {}
        }
    }

    (successes, warnings)
}

/// Returns the raw template and target path for a tool's AGENTS.md, if applicable.
/// The template contains a `{{MAG_VERSION}}` placeholder replaced at install time.
pub(crate) fn agents_md_target(
    tool: tool_detection::AiTool,
    home: &Path,
) -> Option<(&'static str, PathBuf)> {
    match tool {
        tool_detection::AiTool::Codex => Some((AGENTS_MD_TEMPLATE, home.join(".codex/AGENTS.md"))),
        tool_detection::AiTool::GeminiCli => {
            Some((AGENTS_MD_TEMPLATE, home.join(".gemini/AGENTS.md")))
        }
        _ => None,
    }
}

/// Appends the MAG section to `existing` content after a blank line separator,
/// then writes it atomically to `path`. Returns `Ok(true)` (content always changed).
fn install_agents_md_append(existing: &str, content: &str, path: &Path) -> Result<bool> {
    let mut result = existing.to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result.push('\n');
    result.push_str(content);
    if !content.ends_with('\n') {
        result.push('\n');
    }
    atomic_write(path, &result)?;
    Ok(true)
}

/// Installs or updates the MAG section in a tool's AGENTS.md file.
///
/// Uses sentinel comments (`<!-- MAG_MEMORY_START -->` / `<!-- MAG_MEMORY_END -->`)
/// for idempotent append/replace.
///
/// Returns `Ok(true)` if the file was created or updated, `Ok(false)` if already
/// up-to-date (identical content).
pub(crate) fn install_agents_md(tool: tool_detection::AiTool, home: &Path) -> Result<bool> {
    let Some((_, target_path)) = agents_md_target(tool, home) else {
        return Ok(false);
    };
    // Substitute {{MAG_VERSION}} so stale installs are detectable after upgrades.
    let content_owned = agents_md_content();
    let content = content_owned.as_str();

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }

    let existing = match std::fs::read_to_string(&target_path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("reading existing AGENTS.md"),
    };

    let has_start = existing.find(MAG_SENTINEL_START);
    // Search for END only after START to avoid matching an orphan END before a
    // valid START/END pair.
    let has_end = has_start
        .and_then(|si| existing[si..].find(MAG_SENTINEL_END).map(|i| si + i))
        .or_else(|| existing.find(MAG_SENTINEL_END));

    let new_content = if existing.is_empty() {
        content.to_string()
    } else if let Some(start_idx) = has_start {
        // START sentinel found — END must also be present and after START.
        let end_raw = match has_end {
            Some(i) if i >= start_idx => i,
            Some(_) => {
                // END appears before START — malformed. Treat as "no sentinel" and append.
                return install_agents_md_append(&existing, content, &target_path);
            }
            None => {
                anyhow::bail!(
                    "corrupt AGENTS.md: found MAG_MEMORY_START but no matching MAG_MEMORY_END in {}",
                    target_path.display()
                );
            }
        };

        // Replace the existing MAG section in place.
        let end_idx = end_raw + MAG_SENTINEL_END.len();
        let end_idx = if existing[end_idx..].starts_with('\n') {
            end_idx + 1
        } else {
            end_idx
        };

        let mut result = String::with_capacity(existing.len());
        result.push_str(&existing[..start_idx]);
        result.push_str(content);
        if !content.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&existing[end_idx..]);
        result
    } else {
        // No valid START sentinel (either no sentinels, or orphan END only) — append.
        return install_agents_md_append(&existing, content, &target_path);
    };

    if new_content == existing {
        return Ok(false);
    }

    atomic_write(&target_path, &new_content)?;

    Ok(true)
}

/// Installs SKILL.md files for OpenCode (ContentTier::Skills tools).
///
/// Creates `~/.config/opencode/skills/{skill_name}/SKILL.md` for each skill.
/// Returns the number of files created or updated, or `0` if this tool does
/// not use the Skills content tier.
pub(crate) fn install_skills(tool: tool_detection::AiTool, home: &Path) -> Result<usize> {
    if tool.content_tier() != ContentTier::Skills {
        return Ok(0);
    }

    let skills_root = crate::app_paths::xdg_config_home(home).join("opencode/skills");
    let mut errors: Vec<String> = Vec::new();
    let mut count = 0usize;

    for &(skill_name, skill_content) in OPENCODE_SKILLS {
        let skill_dir = skills_root.join(skill_name);
        let skill_path = skill_dir.join("SKILL.md");

        if let Ok(existing) = std::fs::read_to_string(&skill_path)
            && existing == skill_content
        {
            continue;
        }

        if let Err(e) = atomic_write(&skill_path, skill_content) {
            errors.push(format!("{}: {}", skill_name, e));
            continue;
        }
        count += 1;
    }

    if !errors.is_empty() {
        anyhow::bail!(
            "Failed to install {} skill(s): {}",
            errors.len(),
            errors.join("; ")
        );
    }

    Ok(count)
}

/// Removes the MAG sentinel section from an AGENTS.md file at the given path.
///
/// Returns `Ok(true)` if the section was removed, `Ok(false)` if not present.
pub(crate) fn remove_agents_md_section(path: &Path) -> Result<bool> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).context("reading AGENTS.md for removal"),
    };

    let Some(start_idx) = existing.find(MAG_SENTINEL_START) else {
        return Ok(false);
    };

    // Search for END starting from START to avoid matching an orphan END
    // that appears before the START sentinel.
    let Some(end_raw) = existing[start_idx..]
        .find(MAG_SENTINEL_END)
        .map(|i| start_idx + i)
    else {
        anyhow::bail!(
            "AGENTS.md has MAG_MEMORY_START but no matching MAG_MEMORY_END: {}",
            path.display()
        );
    };

    let end_idx = end_raw + MAG_SENTINEL_END.len();

    // Consume the newline immediately after the END sentinel.
    let end_idx = if existing[end_idx..].starts_with('\n') {
        end_idx + 1
    } else {
        end_idx
    };

    // If the text before the sentinel ends with a blank line (the separator
    // written by install_agents_md), consume the extra newline.
    let start_idx = if existing[..start_idx].ends_with("\n\n") {
        start_idx - 1
    } else {
        start_idx
    };

    let mut result = String::with_capacity(existing.len());
    result.push_str(&existing[..start_idx]);
    result.push_str(&existing[end_idx..]);

    if result.trim().is_empty() {
        std::fs::remove_file(path).with_context(|| format!("removing empty {}", path.display()))?;
    } else {
        atomic_write(path, &result)?;
    }

    Ok(true)
}

/// Removes OpenCode skill files created by MAG.
///
/// Only removes the managed SKILL.md file from each skill directory. The
/// directory itself is removed only if it is empty afterwards, so any
/// user-added files are preserved.
pub(crate) fn remove_opencode_skills(home: &Path) -> Result<usize> {
    let skills_root = crate::app_paths::xdg_config_home(home).join("opencode/skills");
    let mut count = 0;

    for &(skill_name, _) in OPENCODE_SKILLS {
        let skill_dir = skills_root.join(skill_name);
        let skill_path = skill_dir.join("SKILL.md");
        match std::fs::remove_file(&skill_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("removing {}", skill_path.display()));
            }
        }
        count += 1;
        // Only remove directory if empty (don't delete user files)
        if std::fs::read_dir(&skill_dir)
            .with_context(|| format!("reading {}", skill_dir.display()))?
            .next()
            .is_none()
            && let Err(e) = std::fs::remove_dir(&skill_dir)
        {
            tracing::warn!(
                path = %skill_dir.display(),
                error = %e,
                "failed to remove empty skill directory"
            );
        }
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Daemon management
// ---------------------------------------------------------------------------

#[cfg(feature = "daemon-http")]
fn maybe_start_daemon(port: u16, no_start: bool) -> Result<()> {
    if no_start {
        tracing::debug!("--no-start: skipping daemon check");
        return Ok(());
    }

    // Check if daemon is already running via daemon.json
    match crate::daemon::DaemonInfo::read() {
        Ok(Some(info)) if !info.is_stale() => {
            println!(
                "  MAG daemon already running (pid {}, port {}).\n",
                info.pid, info.port
            );
            return Ok(());
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to read daemon info; assuming not running");
        }
        _ => {}
    }

    println!("  Tip: start the MAG daemon with `mag serve` (port {port}).\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parses a CLI transport string into a `TransportMode`.
pub fn parse_transport(s: &str, port: u16) -> Result<TransportMode> {
    match s.to_lowercase().as_str() {
        "command" | "cmd" => Ok(TransportMode::Command),
        "http" => Ok(TransportMode::Http { port }),
        "stdio" => Ok(TransportMode::Stdio),
        other => {
            anyhow::bail!("unknown transport mode: '{other}' (expected command, http, or stdio)")
        }
    }
}

/// Returns `true` if we detect a CI environment.
fn is_ci() -> bool {
    std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some()
}

/// Returns `true` if stdin appears to be a TTY.
fn is_tty() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;
    use crate::tool_detection::{AiTool, ConfigScope, DetectedTool, MagConfigStatus};
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Transport parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_transport_command() {
        let mode = parse_transport("command", 4242).unwrap();
        assert_eq!(mode, TransportMode::Command);
    }

    #[test]
    fn parse_transport_cmd_alias() {
        let mode = parse_transport("cmd", 4242).unwrap();
        assert_eq!(mode, TransportMode::Command);
    }

    #[test]
    fn parse_transport_http() {
        let mode = parse_transport("http", 9090).unwrap();
        assert_eq!(mode, TransportMode::Http { port: 9090 });
    }

    #[test]
    fn parse_transport_stdio() {
        let mode = parse_transport("stdio", 4242).unwrap();
        assert_eq!(mode, TransportMode::Stdio);
    }

    #[test]
    fn parse_transport_case_insensitive() {
        let mode = parse_transport("HTTP", 8080).unwrap();
        assert_eq!(mode, TransportMode::Http { port: 8080 });
    }

    #[test]
    fn parse_transport_unknown_errors() {
        let result = parse_transport("grpc", 4242);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("grpc"),
            "error should mention the bad input: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // SetupArgs construction
    // -----------------------------------------------------------------------

    #[test]
    fn setup_args_defaults() {
        let args = SetupArgs {
            non_interactive: false,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: false,
            uninstall: false,
            force: false,
        };
        assert!(!args.non_interactive);
        assert!(args.tools.is_none());
        assert_eq!(args.port, 4242);
    }

    // -----------------------------------------------------------------------
    // Tool selection helpers
    // -----------------------------------------------------------------------

    fn make_detected(tool: AiTool, status: MagConfigStatus) -> DetectedTool {
        DetectedTool {
            tool,
            config_path: PathBuf::from("/fake/config.json"),
            scope: ConfigScope::Global,
            mag_status: status,
        }
    }

    #[test]
    fn select_tools_non_interactive_configures_unconfigured() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::NotConfigured),
                make_detected(AiTool::Cursor, MagConfigStatus::Configured),
                make_detected(AiTool::Windsurf, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].tool, AiTool::ClaudeCode);
        assert_eq!(selected[1].tool, AiTool::Windsurf);
    }

    #[test]
    fn select_tools_with_filter() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::NotConfigured),
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
                make_detected(AiTool::Windsurf, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: Some(vec!["cursor".to_string()]),
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tool, AiTool::Cursor);
    }

    #[test]
    fn select_tools_force_includes_configured() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::Configured),
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: true,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn select_tools_all_configured_returns_empty() {
        let result = DetectionResult {
            detected: vec![make_detected(
                AiTool::ClaudeCode,
                MagConfigStatus::Configured,
            )],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert!(selected.is_empty());
    }

    // -----------------------------------------------------------------------
    // CI / TTY detection
    // -----------------------------------------------------------------------

    #[test]
    fn is_ci_checks_env_vars() {
        // In test environment, CI may or may not be set, but the function
        // should not panic.
        let _ = is_ci();
    }

    // -----------------------------------------------------------------------
    // Presentation (smoke tests — ensure no panics)
    // -----------------------------------------------------------------------

    #[test]
    fn present_detection_empty() {
        let result = DetectionResult {
            detected: vec![],
            not_found: vec![AiTool::ClaudeCode],
        };
        present_detection(&result);
    }

    #[test]
    fn present_detection_with_tools() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::Configured),
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
                make_detected(
                    AiTool::Zed,
                    MagConfigStatus::Misconfigured("missing source".to_string()),
                ),
            ],
            not_found: vec![AiTool::Windsurf],
        };
        present_detection(&result);
    }

    #[test]
    fn present_summary_all_variants() {
        let summary = ConfigureSummary {
            written: vec!["Claude Code".to_string()],
            already_current: vec!["Cursor".to_string()],
            unsupported: vec![("Zed".to_string(), "manual editing required".to_string())],
            deferred: vec!["Codex".to_string()],
            errors: vec![("Windsurf".to_string(), "permission denied".to_string())],
        };
        present_summary(&summary);
    }

    // -----------------------------------------------------------------------
    // Integration: configure_tools with temp home
    // -----------------------------------------------------------------------

    #[test]
    fn configure_tools_writes_config() {
        with_temp_home(|home| {
            // Create a Claude Code config file
            let config_path = home.join(".claude.json");
            std::fs::write(&config_path, "{}").unwrap();

            let dt = DetectedTool {
                tool: AiTool::ClaudeCode,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            let tools: Vec<&DetectedTool> = vec![&dt];
            let summary = configure_tools(&tools, TransportMode::Command, &tools).unwrap();

            assert_eq!(summary.written.len(), 1);
            assert!(summary.errors.is_empty());

            // Claude Code may be configured via plugin (if `claude` CLI is available)
            // or via MCP config (if not). Both are valid outcomes.
            let name = &summary.written[0];
            assert!(
                name == "Claude Code" || name == "Claude Code (plugin)",
                "unexpected written entry: {name}"
            );

            if name == "Claude Code" {
                // Verify the MCP config was written (fallback path)
                let content = std::fs::read_to_string(&config_path).unwrap();
                let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
                assert!(parsed["mcpServers"]["mag"].is_object());
            }
        });
    }

    #[test]
    fn configure_tools_idempotent() {
        with_temp_home(|home| {
            // Create a config that already has MAG configured with the absolute binary path
            // that resolve_mag_binary() produces for this temp HOME.
            let config_path = home.join(".cursor/mcp.json");
            std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            let mag_binary = home.join(".mag").join("bin").join("mag");
            let mag_binary_str = mag_binary.to_string_lossy();
            let initial = format!(
                r#"{{"mcpServers":{{"mag":{{"command":"{mag_binary_str}","args":["serve"]}}}}}}"#
            );
            std::fs::write(&config_path, &initial).unwrap();

            let dt = DetectedTool {
                tool: AiTool::Cursor,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::Configured,
            };

            let tools: Vec<&DetectedTool> = vec![&dt];
            let summary = configure_tools(&tools, TransportMode::Command, &tools).unwrap();

            assert_eq!(summary.already_current.len(), 1);
            assert!(summary.written.is_empty());
        });
    }

    #[test]
    fn configure_tools_zed_unsupported() {
        let dt = DetectedTool {
            tool: AiTool::Zed,
            config_path: PathBuf::from("/fake/zed/settings.json"),
            scope: ConfigScope::Global,
            mag_status: MagConfigStatus::NotConfigured,
        };

        let tools: Vec<&DetectedTool> = vec![&dt];
        let summary = configure_tools(&tools, TransportMode::Command, &tools).unwrap();

        assert_eq!(summary.unsupported.len(), 1);
    }

    #[test]
    fn configure_tools_codex_writes_toml() {
        with_temp_home(|home| {
            let config_path = home.join(".codex/config.toml");
            let dt = DetectedTool {
                tool: AiTool::Codex,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            let tools: Vec<&DetectedTool> = vec![&dt];
            let summary = configure_tools(&tools, TransportMode::Command, &tools).unwrap();

            assert!(summary.deferred.is_empty());
            // At least the TOML config is written; connector content (AGENTS.md)
            // may add additional entries to summary.written.
            assert!(
                summary.written.iter().any(|s| s.contains("Codex")),
                "expected Codex to appear in written entries, got: {:?}",
                summary.written
            );
            assert!(config_path.exists(), "expected config.toml to be created");
        });
    }

    // -----------------------------------------------------------------------
    // Integration: full non-interactive setup flow
    // -----------------------------------------------------------------------

    #[test]
    fn full_non_interactive_setup() {
        with_temp_home(|home| {
            // Set up a Cursor config file
            let cursor_dir = home.join(".cursor");
            std::fs::create_dir_all(&cursor_dir).unwrap();
            std::fs::write(cursor_dir.join("mcp.json"), "{}").unwrap();

            // Detect
            let result = detect_phase(None);
            assert!(
                result.detected.iter().any(|d| d.tool == AiTool::Cursor),
                "expected Cursor to be detected"
            );

            // Select non-interactively
            let args = SetupArgs {
                non_interactive: true,
                tools: None,
                transport: TransportMode::Command,
                port: 4242,
                no_start: true,
                uninstall: false,
                force: false,
            };

            let selected = select_tools(&result, &args).unwrap();
            assert!(!selected.is_empty(), "expected at least one tool selected");

            // Configure — pass invocation-scoped tools for connector content
            let scope = invocation_scoped_tools(&result, &args);
            let summary = configure_tools(&selected, TransportMode::Command, &scope).unwrap();
            assert!(
                !summary.written.is_empty() || !summary.already_current.is_empty(),
                "expected at least one tool configured"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Uninstall flow
    // -----------------------------------------------------------------------

    #[test]
    fn uninstall_removes_configured_tools() {
        with_temp_home(|home| {
            // Set up a Claude Code config with MAG
            let config_path = home.join(".claude.json");
            std::fs::write(
                &config_path,
                r#"{"mcpServers":{"mag":{"command":"mag","args":["serve"]},"other":{}}}"#,
            )
            .unwrap();

            // Run uninstall
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(crate::uninstall::run_uninstall(false, true))
                .unwrap();

            // Verify MAG was removed but other config preserved
            let content = std::fs::read_to_string(&config_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(parsed["mcpServers"]["mag"].is_null());
            assert!(parsed["mcpServers"]["other"].is_object());
        });
    }

    #[test]
    fn uninstall_no_tools_detected() {
        with_temp_home(|_home| {
            // No config files exist — should not error
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(crate::uninstall::run_uninstall(false, true))
                .unwrap();
        });
    }

    // -----------------------------------------------------------------------
    // Tool filter matching
    // -----------------------------------------------------------------------

    #[test]
    fn filter_matches_partial_name() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::VSCodeCopilot, MagConfigStatus::NotConfigured),
                make_detected(AiTool::ClaudeCode, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: Some(vec!["vscode".to_string()]),
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tool, AiTool::VSCodeCopilot);
    }

    #[test]
    fn filter_matches_multiple_tools() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
                make_detected(AiTool::Windsurf, MagConfigStatus::NotConfigured),
                make_detected(AiTool::ClaudeCode, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: Some(vec!["cursor".to_string(), "windsurf".to_string()]),
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 2);
        let tool_names: Vec<_> = selected.iter().map(|d| d.tool).collect();
        assert!(tool_names.contains(&AiTool::Cursor));
        assert!(tool_names.contains(&AiTool::Windsurf));
    }

    // -----------------------------------------------------------------------
    // Plugin-related tests
    // -----------------------------------------------------------------------

    #[test]
    fn select_tools_skips_installed_as_plugin() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::InstalledAsPlugin),
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: false,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tool, AiTool::Cursor);
    }

    #[test]
    fn select_tools_force_includes_plugin_installed() {
        let result = DetectionResult {
            detected: vec![make_detected(
                AiTool::ClaudeCode,
                MagConfigStatus::InstalledAsPlugin,
            )],
            not_found: vec![],
        };
        let args = SetupArgs {
            non_interactive: true,
            tools: None,
            transport: TransportMode::Command,
            port: 4242,
            no_start: true,
            uninstall: false,
            force: true,
        };

        let selected = select_tools(&result, &args).unwrap();
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn present_detection_shows_plugin_status() {
        let result = DetectionResult {
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::InstalledAsPlugin),
                make_detected(AiTool::Cursor, MagConfigStatus::NotConfigured),
            ],
            not_found: vec![],
        };
        // Smoke test — should not panic
        present_detection(&result);
    }

    #[test]
    fn present_summary_with_plugin_entry() {
        let summary = ConfigureSummary {
            written: vec!["Claude Code (plugin)".to_string()],
            already_current: vec![],
            unsupported: vec![],
            deferred: vec![],
            errors: vec![],
        };
        // Smoke test — should not panic
        present_summary(&summary);
    }

    // -----------------------------------------------------------------------
    // Connector: install_agents_md
    // -----------------------------------------------------------------------

    #[test]
    fn install_agents_md_creates_file_for_codex() {
        with_temp_home(|home| {
            let result = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(result, "expected file to be created");

            let path = home.join(".codex/AGENTS.md");
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("<!-- MAG_MEMORY_START -->"));
            assert!(content.contains("<!-- MAG_MEMORY_END -->"));
            assert!(content.contains("mag process"));
        });
    }

    #[test]
    fn install_agents_md_creates_file_for_gemini() {
        with_temp_home(|home| {
            let result = install_agents_md(AiTool::GeminiCli, home).unwrap();
            assert!(result, "expected file to be created");

            let path = home.join(".gemini/AGENTS.md");
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("<!-- MAG_MEMORY_START -->"));
            assert!(content.contains("<!-- MAG_MEMORY_END -->"));
        });
    }

    #[test]
    fn install_agents_md_is_idempotent() {
        with_temp_home(|home| {
            // First install
            let first = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(first, "first install should return true");

            // Second install — content is identical
            let second = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(
                !second,
                "second install should return false (already current)"
            );
        });
    }

    #[test]
    fn install_agents_md_replaces_existing_section() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();

            // Write a file with existing user content + old MAG section
            std::fs::write(
                &path,
                "# My Agent\n\nSome user content.\n\n<!-- MAG_MEMORY_START -->\nOLD MAG CONTENT\n<!-- MAG_MEMORY_END -->\n",
            ).unwrap();

            let result = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(result, "should update the MAG section");

            let content = std::fs::read_to_string(&path).unwrap();
            // User content should be preserved
            assert!(content.contains("# My Agent"));
            assert!(content.contains("Some user content."));
            // Old content should be gone
            assert!(!content.contains("OLD MAG CONTENT"));
            // New content should be present
            assert!(content.contains("mag process"));
        });
    }

    #[test]
    fn install_agents_md_appends_to_existing_file() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "# Existing AGENTS.md\n\nSome guidance.\n").unwrap();

            let result = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(result, "should append MAG section");

            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.starts_with("# Existing AGENTS.md"));
            assert!(content.contains("<!-- MAG_MEMORY_START -->"));
            assert!(content.contains("<!-- MAG_MEMORY_END -->"));
        });
    }

    #[test]
    fn install_agents_md_returns_false_for_non_agents_md_tool() {
        with_temp_home(|home| {
            let result = install_agents_md(AiTool::Cursor, home).unwrap();
            assert!(!result, "Cursor is not an AgentsMd tier tool");
        });
    }

    // -----------------------------------------------------------------------
    // Connector: install_skills
    // -----------------------------------------------------------------------

    #[test]
    fn install_skills_returns_zero_for_non_skills_tier() {
        with_temp_home(|home| {
            // Codex is AgentsMd, not Skills
            let count = install_skills(AiTool::Codex, home).unwrap();
            assert_eq!(count, 0);

            // ClaudeCode is Plugin, not Skills
            let count = install_skills(AiTool::ClaudeCode, home).unwrap();
            assert_eq!(count, 0);
        });
    }

    // -----------------------------------------------------------------------
    // Connector: remove_agents_md_section
    // -----------------------------------------------------------------------

    #[test]
    fn remove_agents_md_section_removes_mag_content() {
        with_temp_home(|home| {
            // Install first
            install_agents_md(AiTool::Codex, home).unwrap();
            let path = home.join(".codex/AGENTS.md");
            assert!(path.exists());

            // Remove
            let removed = remove_agents_md_section(&path).unwrap();
            assert!(removed, "should return true when section is removed");

            // File should be gone (was MAG-only content)
            assert!(!path.exists(), "empty file should be deleted");
        });
    }

    #[test]
    fn remove_agents_md_section_preserves_other_content() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                "# User content\n\n<!-- MAG_MEMORY_START -->\nMAG stuff\n<!-- MAG_MEMORY_END -->\n",
            )
            .unwrap();

            let removed = remove_agents_md_section(&path).unwrap();
            assert!(removed);

            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("# User content"));
            assert!(!content.contains("MAG_MEMORY_START"));
        });
    }

    #[test]
    fn remove_agents_md_section_returns_false_when_no_sentinel() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "# No MAG content here\n").unwrap();

            let removed = remove_agents_md_section(&path).unwrap();
            assert!(!removed);
        });
    }

    #[test]
    fn remove_agents_md_section_returns_false_for_missing_file() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            let removed = remove_agents_md_section(&path).unwrap();
            assert!(!removed);
        });
    }

    // -----------------------------------------------------------------------
    // Connector: remove_opencode_skills
    // -----------------------------------------------------------------------

    #[test]
    fn remove_opencode_skills_returns_zero_when_none_exist() {
        with_temp_home(|home| {
            let count = remove_opencode_skills(home).unwrap();
            assert_eq!(count, 0);
        });
    }

    #[test]
    fn remove_opencode_skills_removes_existing_dirs() {
        with_temp_home(|home| {
            // Create some skill directories
            let skills_root = home.join(".config/opencode/skills");
            let skill_dir = skills_root.join("memory-store");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(skill_dir.join("SKILL.md"), "test").unwrap();

            let skill_dir2 = skills_root.join("memory-health");
            std::fs::create_dir_all(&skill_dir2).unwrap();
            std::fs::write(skill_dir2.join("SKILL.md"), "test").unwrap();

            let count = remove_opencode_skills(home).unwrap();
            assert_eq!(count, 2);
            assert!(!skills_root.join("memory-store").exists());
            assert!(!skills_root.join("memory-health").exists());
        });
    }

    // -----------------------------------------------------------------------
    // Connector: uninstall integration
    // -----------------------------------------------------------------------

    #[test]
    fn uninstall_removes_agents_md_sections() {
        with_temp_home(|home| {
            // Install MAG AGENTS.md for Codex
            install_agents_md(AiTool::Codex, home).unwrap();
            let codex_path = home.join(".codex/AGENTS.md");
            assert!(codex_path.exists());

            // Also create a Codex config.toml so it's detected
            std::fs::write(
                home.join(".codex/config.toml"),
                "[mcp_servers.mag]\ncommand = \"mag\"\n",
            )
            .unwrap();

            // Run uninstall
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(crate::uninstall::run_uninstall(false, true))
                .unwrap();

            // The AGENTS.md should be cleaned up (file removed since MAG-only)
            assert!(
                !codex_path.exists(),
                "MAG-only AGENTS.md should be removed by uninstall"
            );
        });
    }

    // -----------------------------------------------------------------------
    // FIX 5: Additional tests for review findings
    // -----------------------------------------------------------------------

    #[test]
    fn install_agents_md_append_then_idempotent() {
        with_temp_home(|home| {
            // Create an existing file with non-MAG content
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "# Pre-existing content\n\nSome other guidance.\n").unwrap();

            // First call: should append MAG section
            let first = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(first, "first install should return true (content changed)");

            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("# Pre-existing content"));
            assert!(content.contains("<!-- MAG_MEMORY_START -->"));
            assert!(content.contains("<!-- MAG_MEMORY_END -->"));

            // Second call: content is identical, should return false
            let second = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(
                !second,
                "second install should return false (already current)"
            );
        });
    }

    #[test]
    fn install_skills_creates_skill_files_for_opencode() {
        with_temp_home(|home| {
            let count = install_skills(AiTool::OpenCode, home).unwrap();
            assert_eq!(count, 4, "expected 4 SKILL.md files to be created");

            let skills_root = home.join(".config/opencode/skills");
            for dir_name in &[
                "memory-store",
                "memory-recall",
                "memory-checkpoint",
                "memory-health",
            ] {
                let skill_path = skills_root.join(dir_name).join("SKILL.md");
                assert!(skill_path.exists(), "expected {dir_name}/SKILL.md to exist");
                let content = std::fs::read_to_string(&skill_path).unwrap();
                assert!(!content.is_empty(), "SKILL.md for {dir_name} is empty");
            }

            // Idempotent: second call should create 0 new files
            let count2 = install_skills(AiTool::OpenCode, home).unwrap();
            assert_eq!(count2, 0, "second install should be idempotent");
        });
    }

    #[test]
    fn install_agents_md_errors_on_start_without_end_sentinel() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Write a file with START but no END sentinel — corrupt state
            std::fs::write(
                &path,
                "# Content\n\n<!-- MAG_MEMORY_START -->\nOrphaned content\n",
            )
            .unwrap();

            let result = install_agents_md(AiTool::Codex, home);
            assert!(result.is_err(), "expected error for missing END sentinel");
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("MAG_MEMORY_START") && err_msg.contains("MAG_MEMORY_END"),
                "error message should mention both sentinels: {err_msg}"
            );
        });
    }

    #[test]
    fn remove_agents_md_returns_error_when_start_without_end_sentinel() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                "# Content\n\n<!-- MAG_MEMORY_START -->\nOrphaned content\n",
            )
            .unwrap();

            // START without END is malformed — returns an error so the user
            // knows the file needs manual repair (mirrors install-path behaviour).
            let result = remove_agents_md_section(&path);
            assert!(result.is_err(), "expected error for missing END sentinel");
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("MAG_MEMORY_START") && err_msg.contains("MAG_MEMORY_END"),
                "error message should mention both sentinels: {err_msg}"
            );
        });
    }

    #[test]
    fn install_agents_md_appends_when_end_before_start() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Malformed: END before START
            std::fs::write(
                &path,
                "# Content\n<!-- MAG_MEMORY_END -->\n<!-- MAG_MEMORY_START -->\n",
            )
            .unwrap();

            // Should treat as "no sentinel" and append
            let result = install_agents_md(AiTool::Codex, home).unwrap();
            assert!(result, "should append when sentinels are reversed");
        });
    }

    #[test]
    fn remove_agents_md_returns_error_when_end_before_start() {
        with_temp_home(|home| {
            let path = home.join(".codex/AGENTS.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Malformed: END before START — START exists but END is not found
            // after START, so the same "no matching END" error fires.
            std::fs::write(
                &path,
                "# Content\n<!-- MAG_MEMORY_END -->\n<!-- MAG_MEMORY_START -->\n",
            )
            .unwrap();

            let result = remove_agents_md_section(&path);
            assert!(result.is_err(), "expected error for reversed sentinels");
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("MAG_MEMORY_START") && err_msg.contains("MAG_MEMORY_END"),
                "error message should mention both sentinels: {err_msg}"
            );
        });
    }
}
