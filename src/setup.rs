//! Interactive setup wizard for configuring AI coding tools to use MAG.
//!
//! The `mag setup` subcommand detects installed AI tools, presents their
//! configuration status, and writes MCP config entries so that each tool
//! can communicate with the MAG daemon.

use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::config_writer::{self, ConfigWriteResult, RemoveResult, TransportMode};
use crate::tool_detection::{self, DetectedTool, DetectionResult, MagConfigStatus};

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
        return run_uninstall(None);
    }

    // Detect phase
    println!("\n  Detecting AI coding tools...\n");
    let result: DetectionResult =
        tokio::task::spawn_blocking(|| detect_phase(None))
            .await
            .context("tool detection task panicked")?;

    present_detection(&result);

    // Determine which tools to configure
    let tools_to_configure = select_tools(&result, &args)?;

    if tools_to_configure.is_empty() {
        println!("  No tools to configure.");
        return Ok(());
    }

    // Configure phase
    let summary = configure_tools(&tools_to_configure, args.transport)?;
    present_summary(&summary);

    // Daemon phase
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
            MagConfigStatus::Configured => "\u{2713}",          // check mark
            MagConfigStatus::NotConfigured => "\u{2717}",       // X mark
            MagConfigStatus::Misconfigured(_) => "\u{26a0}",    // warning
            MagConfigStatus::Unreadable(_) => "\u{26a0}",       // warning
        };
        let status_label = match &dt.mag_status {
            MagConfigStatus::Configured => "configured",
            MagConfigStatus::NotConfigured => "not configured",
            MagConfigStatus::Misconfigured(reason) => reason.as_str(),
            MagConfigStatus::Unreadable(reason) => reason.as_str(),
        };
        println!(
            "    {status_icon} {name:<20} {status_label}",
            name = dt.tool.display_name(),
        );
        tracing::debug!(
            tool = %dt.tool.display_name(),
            path = %dt.config_path.display(),
            "detected tool"
        );
    }

    if !result.not_found.is_empty() {
        println!();
        let not_found_names: Vec<&str> = result.not_found.iter().map(|t: &tool_detection::AiTool| t.display_name()).collect();
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

fn select_tools<'a>(
    result: &'a DetectionResult,
    args: &SetupArgs,
) -> Result<Vec<&'a DetectedTool>> {
    // Filter by --tools if provided
    let candidates: Vec<&DetectedTool> = if let Some(ref tool_names) = args.tools {
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
    };

    // In force mode, configure all matched tools regardless of status
    if args.force {
        return Ok(candidates);
    }

    // Filter to only unconfigured/misconfigured tools
    let actionable: Vec<&DetectedTool> = candidates
        .into_iter()
        .filter(|dt| !matches!(dt.mag_status, MagConfigStatus::Configured))
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

fn select_tools_interactive<'a>(
    tools: &[&'a DetectedTool],
) -> Result<Vec<&'a DetectedTool>> {
    println!(
        "  Configure {} tool{}? [Y/n] ",
        tools.len(),
        if tools.len() == 1 { "" } else { "s" }
    );
    io::stdout().flush().context("flushing stdout")?;

    let stdin = io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("reading user input")?;

    let trimmed = line.trim().to_lowercase();
    if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
        Ok(tools.to_vec())
    } else {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Configuration phase
// ---------------------------------------------------------------------------

fn configure_tools(
    tools: &[&DetectedTool],
    mode: TransportMode,
) -> Result<ConfigureSummary> {
    let mut summary = ConfigureSummary::default();

    for tool in tools {
        let name = tool.tool.display_name().to_string();
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
            Err(e) => {
                summary.errors.push((name, format!("{e:#}")));
            }
        }
    }

    Ok(summary)
}

// ---------------------------------------------------------------------------
// Uninstall
// ---------------------------------------------------------------------------

fn run_uninstall(project_root: Option<&Path>) -> Result<()> {
    println!("\n  Removing MAG from all detected tools...\n");

    let result = detect_phase(project_root);

    if result.detected.is_empty() {
        println!("  No tools detected — nothing to remove.");
        return Ok(());
    }

    let mut removed = Vec::new();
    let mut not_present = Vec::new();
    let mut errors = Vec::new();

    for dt in &result.detected {
        let name = dt.tool.display_name().to_string();
        match config_writer::remove_config(dt) {
            Ok(RemoveResult::Removed) => removed.push(name),
            Ok(RemoveResult::NotPresent | RemoveResult::NoConfigFile) => {
                not_present.push(name);
            }
            Ok(RemoveResult::UnsupportedFormat { reason }) => {
                not_present.push(format!("{name} (skipped: {reason})"));
            }
            Err(e) => errors.push((name, format!("{e:#}"))),
        }
    }

    println!("  Uninstall summary:\n");
    for name in &removed {
        println!("    \u{2713} {name} — removed");
    }
    for name in &not_present {
        println!("    - {name} — was not configured");
    }
    for (name, err) in &errors {
        println!("    \u{2717} {name} — error: {err}");
    }
    println!();

    Ok(())
}

// ---------------------------------------------------------------------------
// Daemon management
// ---------------------------------------------------------------------------

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
        _ => {}
    }

    println!(
        "  Tip: start the MAG daemon with `mag serve` (port {port}).\n"
    );

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
        other => anyhow::bail!("unknown transport mode: '{other}' (expected command, http, or stdio)"),
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
        assert!(msg.contains("grpc"), "error should mention the bad input: {msg}");
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
            detected: vec![
                make_detected(AiTool::ClaudeCode, MagConfigStatus::Configured),
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
            let summary = configure_tools(&tools, TransportMode::Command).unwrap();

            assert_eq!(summary.written.len(), 1);
            assert_eq!(summary.written[0], "Claude Code");
            assert!(summary.errors.is_empty());

            // Verify the config was written
            let content = std::fs::read_to_string(&config_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(parsed["mcpServers"]["mag"].is_object());
        });
    }

    #[test]
    fn configure_tools_idempotent() {
        with_temp_home(|home| {
            // Create a config that already has MAG configured
            let config_path = home.join(".cursor/mcp.json");
            std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            std::fs::write(
                &config_path,
                r#"{"mcpServers":{"mag":{"command":"mag","args":["serve"]}}}"#,
            )
            .unwrap();

            let dt = DetectedTool {
                tool: AiTool::Cursor,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::Configured,
            };

            let tools: Vec<&DetectedTool> = vec![&dt];
            let summary = configure_tools(&tools, TransportMode::Command).unwrap();

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
        let summary = configure_tools(&tools, TransportMode::Command).unwrap();

        assert_eq!(summary.unsupported.len(), 1);
    }

    #[test]
    fn configure_tools_codex_deferred() {
        let dt = DetectedTool {
            tool: AiTool::Codex,
            config_path: PathBuf::from("/fake/codex/config.toml"),
            scope: ConfigScope::Global,
            mag_status: MagConfigStatus::NotConfigured,
        };

        let tools: Vec<&DetectedTool> = vec![&dt];
        let summary = configure_tools(&tools, TransportMode::Command).unwrap();

        assert_eq!(summary.deferred.len(), 1);
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

            // Configure
            let summary = configure_tools(&selected, TransportMode::Command).unwrap();
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
            run_uninstall(None).unwrap();

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
            run_uninstall(None).unwrap();
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
}
