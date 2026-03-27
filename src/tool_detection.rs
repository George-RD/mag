//! AI tool detection module.
//!
//! Discovers which AI coding tools are installed on the user's system by probing
//! known configuration file paths. This module is synchronous, read-only, and
//! performs no writes.
//!
//! **Canonical types:** [`AiTool`], [`DetectedTool`], [`DetectionResult`], and
//! [`MagConfigStatus`] are defined here and imported by `config_writer` and `setup`.
//! No other module may redefine these types.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Identifies a supported AI coding tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AiTool {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    VSCodeCopilot,
    Windsurf,
    Cline,
    Zed,
    Codex,
    GeminiCli,
}

impl AiTool {
    /// Returns a human-readable display name for this tool.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::VSCodeCopilot => "VS Code + Copilot",
            Self::Windsurf => "Windsurf",
            Self::Cline => "Cline",
            Self::Zed => "Zed",
            Self::Codex => "Codex",
            Self::GeminiCli => "Gemini CLI",
        }
    }

    /// Returns all known tool variants.
    pub fn all() -> &'static [AiTool] {
        static ALL_TOOLS: [AiTool; 9] = [
            AiTool::ClaudeCode,
            AiTool::ClaudeDesktop,
            AiTool::Cursor,
            AiTool::VSCodeCopilot,
            AiTool::Windsurf,
            AiTool::Cline,
            AiTool::Zed,
            AiTool::Codex,
            AiTool::GeminiCli,
        ];
        &ALL_TOOLS
    }

    /// Returns the config format this tool uses for MCP configuration.
    pub fn config_format(&self) -> ConfigFormat {
        match self {
            Self::Codex => ConfigFormat::Toml,
            _ => ConfigFormat::Json,
        }
    }
}

impl fmt::Display for AiTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Config file format used by a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
    Toml,
}

/// The scope (global vs project-local) of a detected config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConfigScope {
    /// User-wide config (in home directory or app support directory).
    Global,
    /// Project-scoped config (relative to a project root).
    Project,
}

/// Whether MAG is already configured in a detected tool's config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MagConfigStatus {
    /// MAG entry found in `mcpServers` (or tool-equivalent key).
    Configured,
    /// Config file exists and is readable, but no MAG entry found.
    NotConfigured,
    /// MAG entry exists but is structurally invalid for the target tool.
    /// For example, a Zed entry missing the required `"source": "custom"` field.
    Misconfigured(String),
    /// Config file exists but could not be read or parsed.
    Unreadable(String),
}

/// Information about a single detected AI tool installation.
#[derive(Debug, Clone, Serialize)]
pub struct DetectedTool {
    /// Which tool this is.
    pub tool: AiTool,
    /// The config file path that was found.
    pub config_path: PathBuf,
    /// Whether this is a global or project-scoped config.
    pub scope: ConfigScope,
    /// Whether MAG is already configured in this tool.
    pub mag_status: MagConfigStatus,
}

/// The result of scanning for all AI tools.
#[derive(Debug, Clone, Serialize)]
pub struct DetectionResult {
    /// Tools that were found installed (may have multiple entries per tool
    /// if both global and project configs exist).
    pub detected: Vec<DetectedTool>,
    /// Tools that were not found at any checked path.
    pub not_found: Vec<AiTool>,
}

#[allow(dead_code)] // Used by tests; kept for future callers.
impl DetectionResult {
    /// Returns tools that are installed but do not have MAG configured.
    pub fn unconfigured(&self) -> Vec<&DetectedTool> {
        self.detected
            .iter()
            .filter(|d| d.mag_status == MagConfigStatus::NotConfigured)
            .collect()
    }

    /// Returns true if any tool has MAG configured.
    pub fn any_configured(&self) -> bool {
        self.detected
            .iter()
            .any(|d| d.mag_status == MagConfigStatus::Configured)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scans the filesystem for all supported AI tools.
///
/// Checks both global (home-directory) and project-local paths.
/// `project_root` is optional; if `None`, project-local detection is skipped.
///
/// This function is synchronous and read-only. It never modifies the filesystem.
///
/// Async callers should invoke this via `tokio::task::spawn_blocking` to avoid
/// blocking the async executor.
pub fn detect_all_tools(project_root: Option<&Path>) -> DetectionResult {
    let home = match crate::app_paths::home_dir() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("HOME/USERPROFILE not set; skipping tool detection");
            return DetectionResult {
                detected: vec![],
                not_found: AiTool::all().to_vec(),
            };
        }
    };

    let mut detected = Vec::new();
    let mut not_found = Vec::new();

    for &tool in AiTool::all() {
        let entries = detect_tool_with_home(tool, &home, project_root);
        if entries.is_empty() {
            not_found.push(tool);
        } else {
            detected.extend(entries);
        }
    }

    DetectionResult {
        detected,
        not_found,
    }
}

/// Checks whether a specific AI tool is installed and returns its detection info.
///
/// Returns a `Vec<DetectedTool>` with all found config locations for this tool
/// (e.g., both global and project-level). Returns an empty `Vec` if the tool
/// is not found at any of its known paths.
#[allow(dead_code)] // Used by tests; kept for future callers.
pub fn detect_tool(tool: AiTool, project_root: Option<&Path>) -> Vec<DetectedTool> {
    let home = match crate::app_paths::home_dir() {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("HOME/USERPROFILE not set; skipping detection for {}", tool);
            return vec![];
        }
    };
    detect_tool_with_home(tool, &home, project_root)
}

/// Returns the JSON key path segments where MCP servers are declared for a given tool.
///
/// This function is not called for TOML-format tools (Codex); the Codex key
/// lookup is handled entirely inside `check_mag_in_toml`.
///
/// Examples:
/// - Claude Code: `&["mcpServers"]`
/// - VS Code: `&["servers"]` (in mcp.json)
/// - Zed: `&["context_servers"]`
pub(crate) fn mcp_key_for_tool(tool: AiTool) -> &'static str {
    match tool {
        AiTool::ClaudeCode
        | AiTool::ClaudeDesktop
        | AiTool::Cursor
        | AiTool::Windsurf
        | AiTool::Cline
        | AiTool::GeminiCli => "mcpServers",
        AiTool::VSCodeCopilot => "servers",
        AiTool::Zed => "context_servers",
        // Codex uses TOML — this key is not used in the detection path,
        // but we return a sensible value for config_writer consumers.
        AiTool::Codex => "mcp_servers",
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Detect a tool given a resolved home directory.
fn detect_tool_with_home(
    tool: AiTool,
    home: &Path,
    project_root: Option<&Path>,
) -> Vec<DetectedTool> {
    let paths = config_paths_for_tool(tool, home, project_root);
    let mut results = Vec::new();

    for (path, scope) in paths {
        tracing::debug!(tool = %tool.display_name(), path = %path.display(), "checking config path");

        if !path.exists() {
            continue;
        }

        let mag_status = match tool.config_format() {
            ConfigFormat::Json => {
                let parent_key = mcp_key_for_tool(tool);
                let server_names = server_names_for_tool(tool);
                check_mag_in_json(&path, server_names, parent_key, tool == AiTool::Zed)
            }
            ConfigFormat::Toml => check_mag_in_toml(&path),
        };

        results.push(DetectedTool {
            tool,
            config_path: path,
            scope,
            mag_status,
        });
    }

    if results.is_empty() {
        tracing::debug!(tool = %tool.display_name(), "not found at any known path");
    }

    results
}

/// Returns the candidate server names to look for in a tool's config.
///
/// Most tools use `["mag"]`. Zed may use `"mag-memory"` or `"mag"`.
fn server_names_for_tool(tool: AiTool) -> &'static [&'static str] {
    match tool {
        AiTool::Zed => &["mag", "mag-memory"],
        _ => &["mag"],
    }
}

/// Returns the list of paths to check for a given tool on the current platform.
/// Paths are returned in priority order (global first, then project).
fn config_paths_for_tool(
    tool: AiTool,
    home: &Path,
    project_root: Option<&Path>,
) -> Vec<(PathBuf, ConfigScope)> {
    let mut paths = Vec::new();

    // Global paths
    for p in global_config_paths(tool, home) {
        paths.push((p, ConfigScope::Global));
    }

    // Project-local paths
    if let Some(root) = project_root {
        for p in project_config_paths(tool, root) {
            paths.push((p, ConfigScope::Project));
        }
    }

    paths
}

/// Returns global config paths for a tool on the current platform.
fn global_config_paths(tool: AiTool, home: &Path) -> Vec<PathBuf> {
    match tool {
        AiTool::ClaudeCode => {
            vec![home.join(".claude.json")]
        }
        AiTool::ClaudeDesktop => {
            if cfg!(target_os = "macos") {
                vec![home.join("Library/Application Support/Claude/claude_desktop_config.json")]
            } else if cfg!(target_os = "windows") {
                appdata_path("Claude/claude_desktop_config.json")
                    .into_iter()
                    .collect()
            } else {
                // Linux / other Unix
                vec![home.join(".config/Claude/claude_desktop_config.json")]
            }
        }
        AiTool::Cursor => {
            vec![home.join(".cursor/mcp.json")]
        }
        AiTool::VSCodeCopilot => {
            if cfg!(target_os = "macos") {
                vec![home.join("Library/Application Support/Code/User/mcp.json")]
            } else if cfg!(target_os = "windows") {
                appdata_path("Code/User/mcp.json").into_iter().collect()
            } else {
                vec![home.join(".config/Code/User/mcp.json")]
            }
        }
        AiTool::Windsurf => {
            vec![home.join(".codeium/windsurf/mcp_config.json")]
        }
        AiTool::Cline => cline_global_paths(home),
        AiTool::Zed => {
            let mut paths = Vec::new();
            if cfg!(target_os = "windows") {
                if let Some(p) = appdata_path("Zed/settings.json") {
                    paths.push(p);
                }
            } else {
                paths.push(home.join(".config/zed/settings.json"));
            }
            paths
        }
        AiTool::Codex => {
            vec![home.join(".codex/config.toml")]
        }
        AiTool::GeminiCli => {
            vec![home.join(".gemini/settings.json")]
        }
    }
}

/// Returns project-local config paths for a tool.
fn project_config_paths(tool: AiTool, project_root: &Path) -> Vec<PathBuf> {
    match tool {
        AiTool::ClaudeCode => {
            vec![project_root.join(".claude/settings.local.json")]
        }
        AiTool::Cursor => {
            vec![project_root.join(".cursor/mcp.json")]
        }
        AiTool::VSCodeCopilot => {
            vec![project_root.join(".vscode/mcp.json")]
        }
        AiTool::Windsurf => {
            vec![project_root.join(".windsurf/mcp.json")]
        }
        AiTool::Zed => {
            vec![project_root.join(".zed/settings.json")]
        }
        AiTool::GeminiCli => {
            vec![project_root.join(".gemini/settings.json")]
        }
        // These tools have no project-level config
        AiTool::ClaudeDesktop | AiTool::Cline | AiTool::Codex => vec![],
    }
}

/// Returns Cline's global config paths across all known host editors.
///
/// Cline stores config in the host editor's `globalStorage` under extension ID
/// `saoudrizwan.claude-dev`. The path varies by host editor (VS Code, Cursor, Windsurf).
fn cline_global_paths(home: &Path) -> Vec<PathBuf> {
    let suffix = "User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json";

    if cfg!(target_os = "macos") {
        let base = home.join("Library/Application Support");
        vec![
            base.join("Code").join(suffix),
            base.join("Cursor").join(suffix),
            base.join("Windsurf").join(suffix),
        ]
    } else if cfg!(target_os = "windows") {
        // On Windows, only the VS Code path is well-known
        appdata_path(&format!("Code/{suffix}"))
            .into_iter()
            .collect()
    } else {
        // Linux
        let base = home.join(".config");
        vec![
            base.join("Code").join(suffix),
            base.join("Cursor").join(suffix),
            base.join("Windsurf").join(suffix),
        ]
    }
}

/// Resolves a path relative to `%APPDATA%` on Windows.
/// Returns `None` if `APPDATA` is not set.
fn appdata_path(relative: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join(relative))
}

/// Reads a JSON config file and checks for a MAG MCP server entry.
///
/// `server_names` supports tools that may use multiple key names (e.g., Zed
/// uses `"mag-memory"` or `"mag"`). All names are checked; first match wins.
///
/// `parent_key` is the top-level JSON key containing the server entries
/// (e.g., `"mcpServers"`, `"servers"`, `"context_servers"`).
///
/// If `validate_zed_source` is true, a found entry is checked for the
/// `"source": "custom"` field required by Zed.
fn check_mag_in_json(
    path: &Path,
    server_names: &[&str],
    parent_key: &str,
    validate_zed_source: bool,
) -> MagConfigStatus {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "failed to read config file");
            return MagConfigStatus::Unreadable(e.to_string());
        }
    };

    // Pre-check: empty file
    if contents.is_empty() {
        return MagConfigStatus::Unreadable("empty config file".to_string());
    }

    let parsed: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "failed to parse JSON config");
            return MagConfigStatus::Unreadable(e.to_string());
        }
    };

    let servers = match parsed.get(parent_key) {
        Some(v) => v,
        None => return MagConfigStatus::NotConfigured,
    };

    // Check each candidate server name
    for &name in server_names {
        if let Some(entry) = servers.get(name) {
            // For Zed, validate that "source": "custom" is present
            if validate_zed_source {
                match entry.get("source").and_then(|v| v.as_str()) {
                    Some("custom") => return MagConfigStatus::Configured,
                    _ => {
                        return MagConfigStatus::Misconfigured(
                            "missing source: custom".to_string(),
                        );
                    }
                }
            }
            return MagConfigStatus::Configured;
        }
    }

    MagConfigStatus::NotConfigured
}

/// Reads a TOML config file (Codex) and checks for a `[mcp_servers.mag]` table.
///
/// Since we don't want to add a TOML parsing dependency, we do a simple
/// string-based check for the section header `[mcp_servers.mag]`.
fn check_mag_in_toml(path: &Path) -> MagConfigStatus {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "failed to read TOML config");
            return MagConfigStatus::Unreadable(e.to_string());
        }
    };

    // Pre-check: empty file
    if contents.is_empty() {
        return MagConfigStatus::Unreadable("empty config file".to_string());
    }

    // Check for the TOML table header [mcp_servers.mag]
    // This is a simple heuristic; a full TOML parser would be more robust.
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[mcp_servers.mag]" {
            return MagConfigStatus::Configured;
        }
    }

    MagConfigStatus::NotConfigured
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    // -- AiTool basics --

    #[test]
    fn all_tools_returns_nine_variants() {
        let all = AiTool::all();
        assert_eq!(all.len(), 9);
        // Each variant appears exactly once
        let set: std::collections::HashSet<AiTool> = all.iter().copied().collect();
        assert_eq!(set.len(), 9);
    }

    #[test]
    fn display_name_is_human_readable() {
        assert_eq!(AiTool::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(AiTool::VSCodeCopilot.display_name(), "VS Code + Copilot");
        assert_eq!(AiTool::GeminiCli.display_name(), "Gemini CLI");
    }

    #[test]
    fn display_trait_matches_display_name() {
        for &tool in AiTool::all() {
            assert_eq!(format!("{tool}"), tool.display_name());
        }
    }

    #[test]
    fn codex_uses_toml_format() {
        assert_eq!(AiTool::Codex.config_format(), ConfigFormat::Toml);
        assert_eq!(AiTool::ClaudeCode.config_format(), ConfigFormat::Json);
    }

    // -- Detection: Claude Code --

    #[test]
    fn detects_claude_code_when_config_exists() {
        with_temp_home(|home| {
            std::fs::write(home.join(".claude.json"), r#"{"mcpServers": {}}"#).unwrap();
            let result = detect_tool(AiTool::ClaudeCode, None);
            assert!(!result.is_empty());
            let detected = &result[0];
            assert_eq!(detected.tool, AiTool::ClaudeCode);
            assert_eq!(detected.mag_status, MagConfigStatus::NotConfigured);
            assert_eq!(detected.scope, ConfigScope::Global);
        });
    }

    #[test]
    fn detects_claude_code_with_mag_configured() {
        with_temp_home(|home| {
            std::fs::write(
                home.join(".claude.json"),
                r#"{"mcpServers": {"mag": {"command": "mag", "args": ["serve"]}}}"#,
            )
            .unwrap();
            let result = detect_tool(AiTool::ClaudeCode, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    #[test]
    fn detects_claude_code_project_config() {
        with_temp_home(|_home| {
            let project =
                std::env::temp_dir().join(format!("mag-project-{}", uuid::Uuid::new_v4()));
            let claude_dir = project.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(
                claude_dir.join("settings.local.json"),
                r#"{"mcpServers": {}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::ClaudeCode, Some(&project));
            assert!(!result.is_empty());
            let project_entry = result.iter().find(|d| d.scope == ConfigScope::Project);
            assert!(project_entry.is_some());

            let _ = std::fs::remove_dir_all(&project);
        });
    }

    // -- Detection: Cursor --

    #[test]
    fn detects_cursor_global_config() {
        with_temp_home(|home| {
            let cursor_dir = home.join(".cursor");
            std::fs::create_dir_all(&cursor_dir).unwrap();
            std::fs::write(cursor_dir.join("mcp.json"), r#"{"mcpServers": {}}"#).unwrap();
            let result = detect_tool(AiTool::Cursor, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].tool, AiTool::Cursor);
            assert_eq!(result[0].mag_status, MagConfigStatus::NotConfigured);
        });
    }

    #[test]
    fn detects_project_local_cursor_config() {
        with_temp_home(|_home| {
            let project =
                std::env::temp_dir().join(format!("mag-project-{}", uuid::Uuid::new_v4()));
            let cursor_dir = project.join(".cursor");
            std::fs::create_dir_all(&cursor_dir).unwrap();
            std::fs::write(cursor_dir.join("mcp.json"), r#"{"mcpServers": {}}"#).unwrap();

            let result = detect_tool(AiTool::Cursor, Some(&project));
            assert!(!result.is_empty());
            assert_eq!(result[0].scope, ConfigScope::Project);

            let _ = std::fs::remove_dir_all(&project);
        });
    }

    // -- Detection: VS Code + Copilot --

    #[test]
    fn detects_vscode_uses_servers_key() {
        with_temp_home(|home| {
            // On non-macOS, non-Windows: ~/.config/Code/User/mcp.json
            // On macOS: ~/Library/Application Support/Code/User/mcp.json
            let config_path = if cfg!(target_os = "macos") {
                let dir = home.join("Library/Application Support/Code/User");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("mcp.json")
            } else {
                let dir = home.join(".config/Code/User");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("mcp.json")
            };
            std::fs::write(&config_path, r#"{"servers": {"mag": {"command": "mag"}}}"#).unwrap();

            let result = detect_tool(AiTool::VSCodeCopilot, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    #[test]
    fn vscode_not_configured_without_mag_entry() {
        with_temp_home(|home| {
            let config_path = if cfg!(target_os = "macos") {
                let dir = home.join("Library/Application Support/Code/User");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("mcp.json")
            } else {
                let dir = home.join(".config/Code/User");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("mcp.json")
            };
            std::fs::write(&config_path, r#"{"servers": {}}"#).unwrap();

            let result = detect_tool(AiTool::VSCodeCopilot, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::NotConfigured);
        });
    }

    // -- Detection: Windsurf --

    #[test]
    fn detects_windsurf_global_config() {
        with_temp_home(|home| {
            let ws_dir = home.join(".codeium/windsurf");
            std::fs::create_dir_all(&ws_dir).unwrap();
            std::fs::write(
                ws_dir.join("mcp_config.json"),
                r#"{"mcpServers": {"mag": {"command": "mag"}}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::Windsurf, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    // -- Detection: Zed --

    #[test]
    fn detects_zed_with_source_custom_validation() {
        with_temp_home(|home| {
            let zed_dir = home.join(".config/zed");
            std::fs::create_dir_all(&zed_dir).unwrap();
            std::fs::write(
                zed_dir.join("settings.json"),
                r#"{"context_servers": {"mag": {"source": "custom", "command": {"path": "mag", "args": ["serve", "--stdio"]}}}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::Zed, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    #[test]
    fn detects_zed_missing_source_custom_as_misconfigured() {
        with_temp_home(|home| {
            let zed_dir = home.join(".config/zed");
            std::fs::create_dir_all(&zed_dir).unwrap();
            std::fs::write(
                zed_dir.join("settings.json"),
                r#"{"context_servers": {"mag": {"command": {"path": "mag", "args": ["serve", "--stdio"]}}}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::Zed, None);
            assert!(!result.is_empty());
            assert!(matches!(
                result[0].mag_status,
                MagConfigStatus::Misconfigured(_)
            ));
        });
    }

    #[test]
    fn detects_zed_with_mag_memory_key() {
        with_temp_home(|home| {
            let zed_dir = home.join(".config/zed");
            std::fs::create_dir_all(&zed_dir).unwrap();
            std::fs::write(
                zed_dir.join("settings.json"),
                r#"{"context_servers": {"mag-memory": {"source": "custom", "command": {"path": "mag"}}}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::Zed, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    // -- Detection: Codex --

    #[test]
    fn detects_codex_with_mag_in_toml() {
        with_temp_home(|home| {
            let codex_dir = home.join(".codex");
            std::fs::create_dir_all(&codex_dir).unwrap();
            std::fs::write(
                codex_dir.join("config.toml"),
                "[mcp_servers.mag]\ncommand = \"mag\"\nargs = [\"serve\"]\n",
            )
            .unwrap();

            let result = detect_tool(AiTool::Codex, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    #[test]
    fn detects_codex_not_configured() {
        with_temp_home(|home| {
            let codex_dir = home.join(".codex");
            std::fs::create_dir_all(&codex_dir).unwrap();
            std::fs::write(
                codex_dir.join("config.toml"),
                "[some_other_section]\nkey = \"value\"\n",
            )
            .unwrap();

            let result = detect_tool(AiTool::Codex, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::NotConfigured);
        });
    }

    // -- Detection: Gemini CLI --

    #[test]
    fn detects_gemini_cli_global_config() {
        with_temp_home(|home| {
            let gemini_dir = home.join(".gemini");
            std::fs::create_dir_all(&gemini_dir).unwrap();
            std::fs::write(gemini_dir.join("settings.json"), r#"{"mcpServers": {}}"#).unwrap();

            let result = detect_tool(AiTool::GeminiCli, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].tool, AiTool::GeminiCli);
            assert_eq!(result[0].mag_status, MagConfigStatus::NotConfigured);
        });
    }

    // -- Detection: Claude Desktop --

    #[test]
    fn detects_claude_desktop_global_config() {
        with_temp_home(|home| {
            let config_path = if cfg!(target_os = "macos") {
                let dir = home.join("Library/Application Support/Claude");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("claude_desktop_config.json")
            } else {
                let dir = home.join(".config/Claude");
                std::fs::create_dir_all(&dir).unwrap();
                dir.join("claude_desktop_config.json")
            };
            std::fs::write(
                &config_path,
                r#"{"mcpServers": {"mag": {"command": "mag"}}}"#,
            )
            .unwrap();

            let result = detect_tool(AiTool::ClaudeDesktop, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].mag_status, MagConfigStatus::Configured);
        });
    }

    // -- Detection: Cline --

    #[test]
    fn detects_cline_in_vscode() {
        with_temp_home(|home| {
            let suffix =
                "User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json";
            let config_path = if cfg!(target_os = "macos") {
                let dir = home.join("Library/Application Support/Code");
                let full = dir.join(suffix);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                full
            } else {
                let dir = home.join(".config/Code");
                let full = dir.join(suffix);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                full
            };
            std::fs::write(&config_path, r#"{"mcpServers": {}}"#).unwrap();

            let result = detect_tool(AiTool::Cline, None);
            assert!(!result.is_empty());
            assert_eq!(result[0].tool, AiTool::Cline);
            assert_eq!(result[0].mag_status, MagConfigStatus::NotConfigured);
        });
    }

    // -- Negative tests --

    #[test]
    fn returns_empty_when_tool_not_installed() {
        with_temp_home(|_home| {
            let result = detect_tool(AiTool::Cursor, None);
            assert!(result.is_empty());
        });
    }

    // -- Full scan tests --

    #[test]
    fn detect_all_with_no_tools_installed() {
        with_temp_home(|_home| {
            let result = detect_all_tools(None);
            assert!(result.detected.is_empty());
            assert_eq!(result.not_found.len(), AiTool::all().len());
        });
    }

    #[test]
    fn detect_all_finds_multiple_tools() {
        with_temp_home(|home| {
            // Set up Claude Code
            std::fs::write(home.join(".claude.json"), r#"{"mcpServers": {}}"#).unwrap();

            // Set up Cursor
            let cursor = home.join(".cursor");
            std::fs::create_dir_all(&cursor).unwrap();
            std::fs::write(cursor.join("mcp.json"), r#"{"mcpServers": {}}"#).unwrap();

            let result = detect_all_tools(None);

            // Check that both tools are detected (don't assert exact count,
            // as Cline could add entries if host editor paths coincide)
            let tools_found: Vec<AiTool> = result.detected.iter().map(|d| d.tool).collect();
            assert!(tools_found.contains(&AiTool::ClaudeCode));
            assert!(tools_found.contains(&AiTool::Cursor));
            assert!(result.not_found.contains(&AiTool::Windsurf));
        });
    }

    // -- Error handling tests --

    #[test]
    fn unreadable_config_returns_unreadable_status() {
        with_temp_home(|home| {
            std::fs::write(home.join(".claude.json"), "not valid json{{{").unwrap();

            let result = detect_tool(AiTool::ClaudeCode, None);
            assert!(!result.is_empty());
            assert!(matches!(
                result[0].mag_status,
                MagConfigStatus::Unreadable(_)
            ));
        });
    }

    #[test]
    fn empty_config_file_returns_unreadable() {
        with_temp_home(|home| {
            std::fs::write(home.join(".claude.json"), "").unwrap();

            let result = detect_tool(AiTool::ClaudeCode, None);
            assert!(!result.is_empty());
            assert!(matches!(
                result[0].mag_status,
                MagConfigStatus::Unreadable(_)
            ));
            if let MagConfigStatus::Unreadable(msg) = &result[0].mag_status {
                assert_eq!(msg, "empty config file");
            }
        });
    }

    #[test]
    fn empty_toml_config_returns_unreadable() {
        with_temp_home(|home| {
            let codex_dir = home.join(".codex");
            std::fs::create_dir_all(&codex_dir).unwrap();
            std::fs::write(codex_dir.join("config.toml"), "").unwrap();

            let result = detect_tool(AiTool::Codex, None);
            assert!(!result.is_empty());
            assert!(matches!(
                result[0].mag_status,
                MagConfigStatus::Unreadable(_)
            ));
            if let MagConfigStatus::Unreadable(msg) = &result[0].mag_status {
                assert_eq!(msg, "empty config file");
            }
        });
    }

    // -- DetectionResult helpers --

    #[test]
    fn unconfigured_filters_correctly() {
        let result = DetectionResult {
            detected: vec![
                DetectedTool {
                    tool: AiTool::ClaudeCode,
                    config_path: PathBuf::from("/test/.claude.json"),
                    scope: ConfigScope::Global,
                    mag_status: MagConfigStatus::Configured,
                },
                DetectedTool {
                    tool: AiTool::Cursor,
                    config_path: PathBuf::from("/test/.cursor/mcp.json"),
                    scope: ConfigScope::Global,
                    mag_status: MagConfigStatus::NotConfigured,
                },
            ],
            not_found: vec![],
        };
        let unconfigured = result.unconfigured();
        assert_eq!(unconfigured.len(), 1);
        assert_eq!(unconfigured[0].tool, AiTool::Cursor);
    }

    #[test]
    fn any_configured_returns_true_when_one_configured() {
        let result = DetectionResult {
            detected: vec![DetectedTool {
                tool: AiTool::ClaudeCode,
                config_path: PathBuf::from("/test/.claude.json"),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::Configured,
            }],
            not_found: vec![],
        };
        assert!(result.any_configured());
    }

    #[test]
    fn any_configured_returns_false_when_none_configured() {
        let result = DetectionResult {
            detected: vec![DetectedTool {
                tool: AiTool::Cursor,
                config_path: PathBuf::from("/test/.cursor/mcp.json"),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            }],
            not_found: vec![],
        };
        assert!(!result.any_configured());
    }

    // -- mcp_key_for_tool --

    #[test]
    fn mcp_key_correct_for_each_tool() {
        assert_eq!(mcp_key_for_tool(AiTool::ClaudeCode), "mcpServers");
        assert_eq!(mcp_key_for_tool(AiTool::ClaudeDesktop), "mcpServers");
        assert_eq!(mcp_key_for_tool(AiTool::Cursor), "mcpServers");
        assert_eq!(mcp_key_for_tool(AiTool::VSCodeCopilot), "servers");
        assert_eq!(mcp_key_for_tool(AiTool::Windsurf), "mcpServers");
        assert_eq!(mcp_key_for_tool(AiTool::Cline), "mcpServers");
        assert_eq!(mcp_key_for_tool(AiTool::Zed), "context_servers");
        assert_eq!(mcp_key_for_tool(AiTool::Codex), "mcp_servers");
        assert_eq!(mcp_key_for_tool(AiTool::GeminiCli), "mcpServers");
    }

    // -- config_format --

    #[test]
    fn config_format_correct_for_each_tool() {
        for &tool in AiTool::all() {
            let expected = if tool == AiTool::Codex {
                ConfigFormat::Toml
            } else {
                ConfigFormat::Json
            };
            assert_eq!(tool.config_format(), expected);
        }
    }
}
