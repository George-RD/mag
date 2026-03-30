//! MCP config writer module.
//!
//! Writes, removes, and verifies MCP server configuration for detected AI
//! tools so that MAG is registered as an MCP server without manual editing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tool_detection::{AiTool, ConfigFormat, DetectedTool};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Which transport to configure for the MCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    /// HTTP transport: `{ "type": "http", "url": "http://127.0.0.1:{port}/mcp" }`
    Http { port: u16 },
    /// Command transport: `{ "command": "mag", "args": ["serve"] }`
    Command,
    /// Stdio transport: `{ "command": "mag", "args": ["serve", "--stdio"] }`
    Stdio,
}

impl std::fmt::Display for TransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http { port } => write!(f, "http (port {port})"),
            Self::Command => f.write_str("command"),
            Self::Stdio => f.write_str("stdio"),
        }
    }
}

/// Outcome of a config write operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigWriteResult {
    /// Config was written successfully. `backup_path` is `Some` with the path
    /// to the `.mag.bak` backup if an existing file was backed up before
    /// writing. `None` when creating a new config file from scratch.
    Written { backup_path: Option<PathBuf> },
    /// Config already contained the correct MAG entry; no write was needed.
    AlreadyCurrent,
    /// The tool's config format is not supported for writing in this version.
    UnsupportedFormat { reason: String },
    /// The tool's config write is deferred because serialization support is
    /// not yet implemented (e.g., TOML for Codex).
    Deferred { tool: AiTool },
    /// MAG was installed as a Claude Code plugin via the marketplace.
    Plugin,
}

/// Outcome of a config remove operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveResult {
    /// MAG entry was found and removed from the config file.
    Removed,
    /// No MAG entry was present in the config file (no-op).
    NotPresent,
    /// Config file does not exist (no-op).
    NoConfigFile,
    /// The tool's config format is not supported for removal (e.g., Zed JSONC).
    UnsupportedFormat { reason: String },
}

/// Status of the MAG entry in a tool's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigStatus {
    /// MAG entry is present and matches the expected transport mode.
    Valid { mode: TransportMode },
    /// MAG entry is present but has unexpected values.
    Stale {
        expected: TransportMode,
        actual: String,
    },
    /// No MAG entry found.
    Missing,
    /// Config file does not exist.
    NoConfigFile,
    /// Config file exists but cannot be parsed.
    Malformed { error: String },
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Writes the MAG MCP entry into the given tool's config file.
///
/// - Reads existing config (or starts from `{}`).
/// - Merges the MAG entry at the tool's key path.
/// - Creates a `.mag.bak` backup before every write (when file exists).
/// - Writes atomically via temp file + rename.
/// - Returns `AlreadyCurrent` if no changes needed.
/// - Returns `UnsupportedFormat` for Zed (JSONC).
/// - Returns `Deferred` for Codex (TOML not yet implemented).
pub fn write_config(tool: &DetectedTool, mode: TransportMode) -> Result<ConfigWriteResult> {
    // Zed: always unsupported
    if tool.tool == AiTool::Zed {
        return Ok(ConfigWriteResult::UnsupportedFormat {
            reason: "Zed config requires manual editing".into(),
        });
    }

    // Codex: TOML is deferred
    if tool.tool.config_format() == ConfigFormat::Toml {
        return Ok(ConfigWriteResult::Deferred { tool: tool.tool });
    }

    // Check idempotency first
    let status = verify_config(tool, mode)?;
    if matches!(status, ConfigStatus::Valid { .. }) {
        return Ok(ConfigWriteResult::AlreadyCurrent);
    }

    let path = &tool.config_path;
    let parent_key = crate::tool_detection::mcp_key_for_tool(tool.tool);
    let mag_entry = build_mag_entry(tool.tool, mode);

    // Read existing config or start fresh
    let (mut root, file_existed) = if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config at {}", path.display()))?;
        let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
        let parsed: serde_json::Value = serde_json::from_str(content)
            .with_context(|| format!("parsing config at {}", path.display()))?;
        (parsed, true)
    } else {
        (serde_json::Value::Object(serde_json::Map::new()), false)
    };

    // Navigate to (or create) the parent key and insert/replace the mag entry
    let root_obj = root
        .as_object_mut()
        .with_context(|| format!("config root is not an object at {}", path.display()))?;
    if !root_obj.contains_key(parent_key) {
        root_obj.insert(parent_key.to_string(), serde_json::json!({}));
    }
    let current = root
        .get_mut(parent_key)
        .with_context(|| format!("failed to navigate to key '{parent_key}'"))?;

    // Insert the mag entry
    current
        .as_object_mut()
        .with_context(|| format!("MCP parent key is not an object in {}", path.display()))?
        .insert("mag".to_string(), mag_entry);

    // Create backup if file existed
    let backup_path = if file_existed {
        let bak = backup_path_for(path);
        std::fs::copy(path, &bak)
            .with_context(|| format!("creating backup at {}", bak.display()))?;
        Some(bak)
    } else {
        None
    };

    // Serialize with 2-space indent + trailing newline
    let serialized = serialize_json(&root)?;

    // Atomic write
    atomic_write(path, serialized.as_bytes())?;

    Ok(ConfigWriteResult::Written { backup_path })
}

/// Removes the MAG entry from the given tool's config file.
///
/// Creates a `.mag.bak` backup before removal. Returns `RemoveResult::Removed`
/// if the entry was found and removed, `NotPresent` if no entry existed,
/// `NoConfigFile` if the file does not exist, or `UnsupportedFormat` for Zed.
pub fn remove_config(tool: &DetectedTool) -> Result<RemoveResult> {
    // Zed: unsupported
    if tool.tool == AiTool::Zed {
        return Ok(RemoveResult::UnsupportedFormat {
            reason: "Zed config requires manual editing".into(),
        });
    }

    // Codex: also unsupported for now (TOML)
    if tool.tool.config_format() == ConfigFormat::Toml {
        return Ok(RemoveResult::UnsupportedFormat {
            reason: "Codex TOML config removal is not yet supported".into(),
        });
    }

    let path = &tool.config_path;

    if !path.exists() {
        return Ok(RemoveResult::NoConfigFile);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading config at {}", path.display()))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let mut root: serde_json::Value = serde_json::from_str(content)
        .with_context(|| format!("parsing config at {}", path.display()))?;

    let parent_key = crate::tool_detection::mcp_key_for_tool(tool.tool);

    // Navigate to the parent key
    let current = match root.get_mut(parent_key) {
        Some(v) => v,
        None => return Ok(RemoveResult::NotPresent),
    };

    // Check if mag entry exists
    let parent_map = match current.as_object_mut() {
        Some(m) => m,
        None => return Ok(RemoveResult::NotPresent),
    };

    if parent_map.remove("mag").is_none() {
        return Ok(RemoveResult::NotPresent);
    }

    // Backup before writing
    let bak = backup_path_for(path);
    std::fs::copy(path, &bak).with_context(|| format!("creating backup at {}", bak.display()))?;

    // Serialize and write back
    let serialized = serialize_json(&root)?;
    atomic_write(path, serialized.as_bytes())?;

    Ok(RemoveResult::Removed)
}

/// Checks whether the MAG entry in the tool's config is present and correct
/// for the given transport mode. Does not modify any files.
pub fn verify_config(tool: &DetectedTool, mode: TransportMode) -> Result<ConfigStatus> {
    let path = &tool.config_path;

    if !path.exists() {
        return Ok(ConfigStatus::NoConfigFile);
    }

    // Codex TOML: simple string-based verify
    if tool.tool.config_format() == ConfigFormat::Toml {
        return verify_toml_config(path, mode);
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading config at {}", path.display()))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let root: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            return Ok(ConfigStatus::Malformed {
                error: e.to_string(),
            });
        }
    };

    let parent_key = crate::tool_detection::mcp_key_for_tool(tool.tool);

    // Navigate to the parent key
    let current = match root.get(parent_key) {
        Some(v) => v,
        None => return Ok(ConfigStatus::Missing),
    };

    // Check for mag entry
    let mag_entry = match current.get("mag") {
        Some(v) => v,
        None => return Ok(ConfigStatus::Missing),
    };

    // Compare against expected
    let expected_entry = build_mag_entry(tool.tool, mode);
    if *mag_entry == expected_entry {
        Ok(ConfigStatus::Valid { mode })
    } else {
        Ok(ConfigStatus::Stale {
            expected: mode,
            actual: mag_entry.to_string(),
        })
    }
}

/// Builds the `serde_json::Value` for the MAG MCP entry given a transport mode
/// and the target tool.
///
/// The `tool` parameter is required because some tools (e.g., Zed) use a
/// different entry structure than the generic format.
pub fn build_mag_entry(tool: AiTool, mode: TransportMode) -> serde_json::Value {
    // Zed uses a different structure
    if tool == AiTool::Zed {
        return build_zed_entry();
    }

    match mode {
        TransportMode::Http { port } => {
            serde_json::json!({
                "type": "http",
                "url": format!("http://127.0.0.1:{port}/mcp")
            })
        }
        TransportMode::Command => {
            serde_json::json!({
                "command": "mag",
                "args": ["serve"]
            })
        }
        TransportMode::Stdio => {
            serde_json::json!({
                "command": "mag",
                "args": ["serve", "--stdio"]
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Claude Code plugin (marketplace) API
// ---------------------------------------------------------------------------

/// Installs MAG as a Claude Code plugin via the marketplace.
///
/// Runs:
/// 1. `claude plugin marketplace add George-RD/mag-plugins`
/// 2. `claude plugin install mag@mag-plugins --scope user`
///
/// Returns `ConfigWriteResult::Plugin` on success.
/// Falls back to an error if the `claude` CLI is not found or the install fails.
pub fn install_claude_plugin() -> Result<ConfigWriteResult> {
    let claude = find_claude_cli()?;

    // Step 1: add the marketplace (ignore "already added" errors)
    let add_output = std::process::Command::new(&claude)
        .args(["plugin", "marketplace", "add", "George-RD/mag-plugins"])
        .output()
        .with_context(|| format!("running `{} plugin marketplace add`", claude.display()))?;

    tracing::debug!(
        status = %add_output.status,
        stdout_len = add_output.stdout.len(),
        stderr_len = add_output.stderr.len(),
        "claude plugin marketplace add"
    );

    // Step 2: install the plugin
    let install_output = std::process::Command::new(&claude)
        .args(["plugin", "install", "mag@mag-plugins", "--scope", "user"])
        .output()
        .with_context(|| format!("running `{} plugin install`", claude.display()))?;

    tracing::debug!(
        status = %install_output.status,
        stdout_len = install_output.stdout.len(),
        stderr_len = install_output.stderr.len(),
        "claude plugin install"
    );

    if !install_output.status.success() {
        let stderr = String::from_utf8_lossy(&install_output.stderr);
        let stdout = String::from_utf8_lossy(&install_output.stdout);
        anyhow::bail!(
            "claude plugin install failed (exit {}): {}{}",
            install_output.status,
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!("\n{}", stdout.trim())
            }
        );
    }

    Ok(ConfigWriteResult::Plugin)
}

/// Removes the MAG Claude Code plugin.
///
/// Runs `claude plugin uninstall mag@mag-plugins --scope user`.
pub fn remove_claude_plugin() -> Result<RemoveResult> {
    let claude = find_claude_cli()?;

    let output = std::process::Command::new(&claude)
        .args(["plugin", "uninstall", "mag@mag-plugins", "--scope", "user"])
        .output()
        .with_context(|| format!("running `{} plugin uninstall`", claude.display()))?;

    tracing::debug!(
        status = %output.status,
        stdout_len = output.stdout.len(),
        stderr_len = output.stderr.len(),
        "claude plugin uninstall"
    );

    if output.status.success() {
        Ok(RemoveResult::Removed)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "not found" means plugin wasn't installed — treat as NotPresent.
        // Any other failure is a real error.
        if stderr.contains("not found") {
            tracing::debug!("plugin not installed, treating as not-present");
            Ok(RemoveResult::NotPresent)
        } else {
            anyhow::bail!(
                "claude plugin uninstall failed (exit {}): {}",
                output.status,
                stderr.trim()
            )
        }
    }
}

/// Checks whether the MAG plugin is installed and enabled in Claude Code.
///
/// Reads `~/.claude/settings.json` and looks for `"mag@mag-plugins": true`
/// in the `enabledPlugins` object.
#[allow(dead_code)] // Used by setup.rs and tests; clippy can't trace cross-module usage
pub fn verify_claude_plugin() -> Result<bool> {
    let home = crate::app_paths::home_dir()?;
    let settings_path = home.join(".claude/settings.json");

    if !settings_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("reading {}", settings_path.display()))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);

    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(error = %e, "failed to parse claude settings.json");
            return Ok(false);
        }
    };

    let enabled = parsed
        .get("enabledPlugins")
        .and_then(|v| v.get("mag@mag-plugins"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(enabled)
}

/// Locates the `claude` CLI binary on the system PATH.
///
/// Uses `command -v claude` on Unix (or `where claude` on Windows) to resolve
/// the binary path without requiring an external crate.
fn find_claude_cli() -> Result<PathBuf> {
    let output = if cfg!(target_os = "windows") {
        std::process::Command::new("where").arg("claude").output()
    } else {
        std::process::Command::new("sh")
            .args(["-c", "command -v claude"])
            .output()
    };

    match output {
        Ok(o) if o.status.success() => {
            let path_str = String::from_utf8_lossy(&o.stdout);
            let path = path_str.trim();
            if path.is_empty() {
                anyhow::bail!("claude CLI not found on PATH — install Claude Code first");
            }
            Ok(PathBuf::from(path))
        }
        _ => {
            anyhow::bail!("claude CLI not found on PATH — install Claude Code first");
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Builds the Zed-specific context server entry for MAG.
/// Zed always uses stdio with its specific structure.
fn build_zed_entry() -> serde_json::Value {
    serde_json::json!({
        "source": "custom",
        "command": {
            "path": "mag",
            "args": ["serve", "--stdio"]
        }
    })
}

/// Returns the backup path for a given config file path.
fn backup_path_for(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_os_string();
    bak.push(".mag.bak");
    PathBuf::from(bak)
}

/// Serialize a JSON value with 2-space indent and trailing newline.
fn serialize_json(value: &serde_json::Value) -> Result<String> {
    let buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(buf, formatter);
    serde::Serialize::serialize(value, &mut ser).context("serializing JSON config")?;
    let mut output =
        String::from_utf8(ser.into_inner()).context("JSON serialization produced non-UTF8")?;
    output.push('\n');
    Ok(output)
}

/// Atomically write content to a file using temp + rename.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    // Ensure parent dirs exist
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }

    let pid = std::process::id();
    let tmp_name = format!(
        "{}.mag.{pid}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let tmp_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&tmp_name);

    std::fs::write(&tmp_path, content)
        .with_context(|| format!("writing temp file {}", tmp_path.display()))?;

    match std::fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Check for cross-device link error (EXDEV)
            if is_cross_device_error(&e) {
                tracing::debug!("cross-device rename failed, using same-directory fallback");
                // Temp file is already in the same directory, so this shouldn't
                // happen in practice, but handle it gracefully.
                let same_dir_tmp = path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(format!(
                        "{}.mag.{pid}.xdev.tmp",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                std::fs::copy(&tmp_path, &same_dir_tmp)
                    .with_context(|| format!("cross-device copy to {}", same_dir_tmp.display()))?;
                std::fs::rename(&same_dir_tmp, path).with_context(|| {
                    format!("renaming {} -> {}", same_dir_tmp.display(), path.display())
                })?;
                // Clean up the original temp file
                let _ = std::fs::remove_file(&tmp_path);
                Ok(())
            } else {
                // Clean up temp file on failure
                let _ = std::fs::remove_file(&tmp_path);
                Err(e).with_context(|| {
                    format!("renaming {} -> {}", tmp_path.display(), path.display())
                })
            }
        }
    }
}

/// Check if an IO error is a cross-device link error (EXDEV).
fn is_cross_device_error(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc_exdev())
}

/// Returns the EXDEV error code for the current platform.
fn libc_exdev() -> i32 {
    // EXDEV is 18 on Linux and macOS
    18
}

/// Verify a TOML config (Codex) for an expected transport mode.
fn verify_toml_config(path: &Path, mode: TransportMode) -> Result<ConfigStatus> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading config at {}", path.display()))?;

    if content.is_empty() {
        return Ok(ConfigStatus::Malformed {
            error: "empty config file".into(),
        });
    }

    // Simple string-based check for TOML
    if !content.contains("[mcp_servers.mag]") && !content.contains("mcp_servers.mag") {
        return Ok(ConfigStatus::Missing);
    }

    // The entry exists; try to determine if it matches the expected mode
    match mode {
        TransportMode::Http { port } => {
            let expected_url = format!("http://127.0.0.1:{port}/mcp");
            if content.contains(&expected_url) && content.contains("type = \"http\"") {
                Ok(ConfigStatus::Valid { mode })
            } else {
                Ok(ConfigStatus::Stale {
                    expected: mode,
                    actual: "TOML entry exists but does not match expected HTTP config".into(),
                })
            }
        }
        TransportMode::Command => {
            if content.contains("command = \"mag\"")
                && content.contains("args = [\"serve\"]")
                && !content.contains("\"--stdio\"")
            {
                Ok(ConfigStatus::Valid { mode })
            } else {
                Ok(ConfigStatus::Stale {
                    expected: mode,
                    actual: "TOML entry exists but does not match expected command config".into(),
                })
            }
        }
        TransportMode::Stdio => {
            if content.contains("command = \"mag\"") && content.contains("\"--stdio\"") {
                Ok(ConfigStatus::Valid { mode })
            } else {
                Ok(ConfigStatus::Stale {
                    expected: mode,
                    actual: "TOML entry exists but does not match expected stdio config".into(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;
    use crate::tool_detection::{ConfigScope, DetectedTool, MagConfigStatus};
    use serial_test::serial;

    /// Helper to create a DetectedTool for testing.
    fn make_detected(tool: AiTool, config_path: PathBuf) -> DetectedTool {
        DetectedTool {
            tool,
            config_path,
            scope: ConfigScope::Global,
            mag_status: MagConfigStatus::NotConfigured,
        }
    }

    // -----------------------------------------------------------------------
    // build_mag_entry tests
    // -----------------------------------------------------------------------

    #[test]
    fn build_http_entry() {
        let entry = build_mag_entry(AiTool::ClaudeCode, TransportMode::Http { port: 19420 });
        assert_eq!(entry["type"], "http");
        assert_eq!(entry["url"], "http://127.0.0.1:19420/mcp");
    }

    #[test]
    fn build_command_entry() {
        let entry = build_mag_entry(AiTool::Cursor, TransportMode::Command);
        assert_eq!(entry["command"], "mag");
        assert_eq!(entry["args"], serde_json::json!(["serve"]));
    }

    #[test]
    fn build_stdio_entry() {
        let entry = build_mag_entry(AiTool::Windsurf, TransportMode::Stdio);
        assert_eq!(entry["command"], "mag");
        assert_eq!(entry["args"], serde_json::json!(["serve", "--stdio"]));
    }

    #[test]
    fn build_zed_entry_is_custom() {
        let entry = build_mag_entry(AiTool::Zed, TransportMode::Stdio);
        assert_eq!(entry["source"], "custom");
        assert_eq!(entry["command"]["path"], "mag");
        assert_eq!(
            entry["command"]["args"],
            serde_json::json!(["serve", "--stdio"])
        );
    }

    #[test]
    fn build_http_entry_custom_port() {
        let entry = build_mag_entry(AiTool::ClaudeCode, TransportMode::Http { port: 8080 });
        assert_eq!(entry["url"], "http://127.0.0.1:8080/mcp");
    }

    // -----------------------------------------------------------------------
    // write_config tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn write_then_verify_reports_valid() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let mode = TransportMode::Http { port: 19420 };

            let result = write_config(&detected, mode).expect("write_config should succeed");
            assert!(matches!(result, ConfigWriteResult::Written { .. }));

            let status = verify_config(&detected, mode).expect("verify_config should succeed");
            assert!(matches!(status, ConfigStatus::Valid { .. }));
        });
    }

    #[test]
    #[serial]
    fn write_then_read_preserves_entry() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let mode = TransportMode::Command;

            write_config(&detected, mode).expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            let mag = &parsed["mcpServers"]["mag"];
            assert_eq!(mag["command"], "mag");
            assert_eq!(mag["args"], serde_json::json!(["serve"]));
        });
    }

    #[test]
    #[serial]
    fn existing_servers_preserved_after_write() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let initial = r#"{
  "mcpServers": {
    "other-tool": {
      "command": "other",
      "args": ["run"]
    }
  }
}"#;
            std::fs::write(&config_path, initial).expect("write initial config");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            write_config(&detected, TransportMode::Command).expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            assert_eq!(parsed["mcpServers"]["other-tool"]["command"], "other");
            assert_eq!(parsed["mcpServers"]["mag"]["command"], "mag");
        });
    }

    #[test]
    #[serial]
    fn existing_non_mcp_keys_preserved() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let initial = r#"{"theme": "dark", "mcpServers": {}}"#;
            std::fs::write(&config_path, initial).expect("write initial config");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            write_config(&detected, TransportMode::Command).expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            assert_eq!(parsed["theme"], "dark");
        });
    }

    #[test]
    #[serial]
    fn update_existing_mag_entry() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            // Write with Command mode
            write_config(&detected, TransportMode::Command).expect("write command mode");

            // Write again with Http mode
            let result = write_config(&detected, TransportMode::Http { port: 19420 })
                .expect("write http mode");
            assert!(matches!(result, ConfigWriteResult::Written { .. }));

            // Verify it's now Http
            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            assert_eq!(parsed["mcpServers"]["mag"]["type"], "http");
            assert_eq!(
                parsed["mcpServers"]["mag"]["url"],
                "http://127.0.0.1:19420/mcp"
            );
            // No "command" key should remain
            assert!(parsed["mcpServers"]["mag"]["command"].is_null());
        });
    }

    // -----------------------------------------------------------------------
    // Backup tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn first_write_creates_backup() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let original = r#"{"mcpServers": {}}"#;
            std::fs::write(&config_path, original).expect("write original");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let result =
                write_config(&detected, TransportMode::Command).expect("write should succeed");

            if let ConfigWriteResult::Written {
                backup_path: Some(bak),
            } = result
            {
                let backup_content = std::fs::read_to_string(&bak).expect("read backup");
                assert_eq!(backup_content, original);
            } else {
                panic!("expected Written with backup_path");
            }
        });
    }

    #[test]
    #[serial]
    fn second_write_updates_backup() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let original = r#"{"mcpServers": {}}"#;
            std::fs::write(&config_path, original).expect("write original");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            write_config(&detected, TransportMode::Command).expect("first write");

            // Simulate user editing the file
            let modified =
                r#"{"mcpServers": {"mag": {"command": "mag", "args": ["serve"]}}, "extra": true}"#;
            std::fs::write(&config_path, modified).expect("simulate user edit");

            // Write again (update)
            write_config(&detected, TransportMode::Http { port: 19420 }).expect("second write");

            // Backup should contain the modified version (not the original)
            let bak_path = backup_path_for(&config_path);
            let backup_content = std::fs::read_to_string(&bak_path).expect("read backup");
            assert_eq!(backup_content, modified);
        });
    }

    #[test]
    #[serial]
    fn new_file_write_has_no_backup() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            let result =
                write_config(&detected, TransportMode::Command).expect("write should succeed");
            assert_eq!(result, ConfigWriteResult::Written { backup_path: None });
        });
    }

    // -----------------------------------------------------------------------
    // Idempotency tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn double_write_returns_already_current() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let mode = TransportMode::Command;

            let result1 = write_config(&detected, mode).expect("first write");
            assert!(matches!(result1, ConfigWriteResult::Written { .. }));

            let result2 = write_config(&detected, mode).expect("second write");
            assert_eq!(result2, ConfigWriteResult::AlreadyCurrent);
        });
    }

    #[test]
    #[serial]
    fn double_write_file_unchanged() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let mode = TransportMode::Command;

            write_config(&detected, mode).expect("first write");
            let content_after_first = std::fs::read_to_string(&config_path).expect("read");

            write_config(&detected, mode).expect("second write");
            let content_after_second = std::fs::read_to_string(&config_path).expect("read");

            assert_eq!(content_after_first, content_after_second);
        });
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn write_creates_missing_parent_dirs() {
        with_temp_home(|home| {
            let config_path = home.join("deep/nested/dir/.claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            let result =
                write_config(&detected, TransportMode::Command).expect("write should succeed");
            assert!(matches!(result, ConfigWriteResult::Written { .. }));
            assert!(config_path.exists());
        });
    }

    #[test]
    #[serial]
    fn write_to_nonexistent_file_creates_it() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            assert!(!config_path.exists());

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            write_config(&detected, TransportMode::Command).expect("write should succeed");

            assert!(config_path.exists());
            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            assert_eq!(parsed["mcpServers"]["mag"]["command"], "mag");
        });
    }

    #[test]
    #[serial]
    fn remove_when_no_mag_entry_returns_not_present() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            std::fs::write(&config_path, r#"{"mcpServers": {}}"#).expect("write config");

            let detected = make_detected(AiTool::ClaudeCode, config_path);
            let result = remove_config(&detected).expect("remove should succeed");
            assert_eq!(result, RemoveResult::NotPresent);
        });
    }

    #[test]
    #[serial]
    fn remove_when_no_file_returns_no_config_file() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path);

            let result = remove_config(&detected).expect("remove should succeed");
            assert_eq!(result, RemoveResult::NoConfigFile);
        });
    }

    #[test]
    #[serial]
    fn remove_existing_mag_entry() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            // Write a config first
            write_config(&detected, TransportMode::Command).expect("write");

            // Verify it's there
            let status =
                verify_config(&detected, TransportMode::Command).expect("verify before remove");
            assert!(matches!(status, ConfigStatus::Valid { .. }));

            // Remove it
            let result = remove_config(&detected).expect("remove should succeed");
            assert_eq!(result, RemoveResult::Removed);

            // Verify it's gone
            let status =
                verify_config(&detected, TransportMode::Command).expect("verify after remove");
            assert_eq!(status, ConfigStatus::Missing);
        });
    }

    #[test]
    #[serial]
    fn remove_preserves_other_servers() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let initial = r#"{"mcpServers": {"other-tool": {"command": "other"}, "mag": {"command": "mag", "args": ["serve"]}}}"#;
            std::fs::write(&config_path, initial).expect("write initial");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let result = remove_config(&detected).expect("remove should succeed");
            assert_eq!(result, RemoveResult::Removed);

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse");
            assert_eq!(parsed["mcpServers"]["other-tool"]["command"], "other");
            assert!(parsed["mcpServers"]["mag"].is_null());
        });
    }

    // -----------------------------------------------------------------------
    // verify_config tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn verify_missing_file_returns_no_config_file() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path);

            let status = verify_config(&detected, TransportMode::Command).expect("verify");
            assert_eq!(status, ConfigStatus::NoConfigFile);
        });
    }

    #[test]
    #[serial]
    fn verify_malformed_json_returns_malformed() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            std::fs::write(&config_path, "not valid json{{{").expect("write bad json");

            let detected = make_detected(AiTool::ClaudeCode, config_path);
            let status = verify_config(&detected, TransportMode::Command).expect("verify");
            assert!(matches!(status, ConfigStatus::Malformed { .. }));
        });
    }

    #[test]
    #[serial]
    fn verify_stale_entry_returns_stale() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            // Write with Http mode
            write_config(&detected, TransportMode::Http { port: 19420 }).expect("write http");

            // Verify with Command mode should report Stale
            let status = verify_config(&detected, TransportMode::Command).expect("verify");
            assert!(matches!(status, ConfigStatus::Stale { .. }));

            if let ConfigStatus::Stale { expected, .. } = status {
                assert_eq!(expected, TransportMode::Command);
            }
        });
    }

    #[test]
    #[serial]
    fn verify_missing_mag_entry_returns_missing() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            std::fs::write(&config_path, r#"{"mcpServers": {}}"#).expect("write config");

            let detected = make_detected(AiTool::ClaudeCode, config_path);
            let status = verify_config(&detected, TransportMode::Command).expect("verify");
            assert_eq!(status, ConfigStatus::Missing);
        });
    }

    // -----------------------------------------------------------------------
    // Zed and Codex special cases
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn zed_write_returns_unsupported_format() {
        with_temp_home(|home| {
            let config_path = home.join(".config/zed/settings.json");
            let detected = DetectedTool {
                tool: AiTool::Zed,
                config_path,
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            let result =
                write_config(&detected, TransportMode::Stdio).expect("write should succeed");
            assert!(matches!(
                result,
                ConfigWriteResult::UnsupportedFormat { .. }
            ));
        });
    }

    #[test]
    #[serial]
    fn zed_remove_returns_unsupported_format() {
        with_temp_home(|home| {
            let config_path = home.join(".config/zed/settings.json");
            let detected = DetectedTool {
                tool: AiTool::Zed,
                config_path,
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::Configured,
            };

            let result = remove_config(&detected).expect("remove should succeed");
            assert!(matches!(result, RemoveResult::UnsupportedFormat { .. }));
        });
    }

    #[test]
    #[serial]
    fn codex_write_returns_deferred() {
        with_temp_home(|home| {
            let config_path = home.join(".codex/config.toml");
            let detected = DetectedTool {
                tool: AiTool::Codex,
                config_path,
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            let result =
                write_config(&detected, TransportMode::Command).expect("write should succeed");
            assert!(matches!(result, ConfigWriteResult::Deferred { .. }));
        });
    }

    // -----------------------------------------------------------------------
    // VS Code specific tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn vscode_uses_servers_key() {
        with_temp_home(|home| {
            let config_path = home.join("vscode/mcp.json");
            let detected = DetectedTool {
                tool: AiTool::VSCodeCopilot,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            write_config(&detected, TransportMode::Http { port: 19420 })
                .expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            // VS Code uses "servers", not "mcpServers"
            assert!(parsed.get("servers").is_some());
            assert!(parsed.get("mcpServers").is_none());
            assert_eq!(
                parsed["servers"]["mag"]["url"],
                "http://127.0.0.1:19420/mcp"
            );
        });
    }

    // -----------------------------------------------------------------------
    // BOM handling
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn bom_prefixed_config_is_handled() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let bom_content = "\u{FEFF}{\"mcpServers\": {}}";
            std::fs::write(&config_path, bom_content).expect("write bom config");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let result =
                write_config(&detected, TransportMode::Command).expect("write should succeed");
            assert!(matches!(result, ConfigWriteResult::Written { .. }));

            // Verify the entry was written correctly
            let status = verify_config(&detected, TransportMode::Command).expect("verify");
            assert!(matches!(status, ConfigStatus::Valid { .. }));
        });
    }

    // -----------------------------------------------------------------------
    // Serialization format
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn output_has_two_space_indent_and_trailing_newline() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());

            write_config(&detected, TransportMode::Command).expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            // Should end with newline
            assert!(content.ends_with('\n'));
            // Should use 2-space indentation
            assert!(content.contains("  \"mcpServers\""));
        });
    }

    // -----------------------------------------------------------------------
    // Transport mode Display
    // -----------------------------------------------------------------------

    #[test]
    fn transport_mode_display() {
        assert_eq!(
            TransportMode::Http { port: 19420 }.to_string(),
            "http (port 19420)"
        );
        assert_eq!(TransportMode::Command.to_string(), "command");
        assert_eq!(TransportMode::Stdio.to_string(), "stdio");
    }

    // -----------------------------------------------------------------------
    // Windsurf config test
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn windsurf_write_and_verify() {
        with_temp_home(|home| {
            let config_path = home.join(".codeium/windsurf/mcp_config.json");
            let detected = DetectedTool {
                tool: AiTool::Windsurf,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };

            write_config(&detected, TransportMode::Stdio).expect("write should succeed");

            let content = std::fs::read_to_string(&config_path).expect("read config");
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse json");
            assert_eq!(parsed["mcpServers"]["mag"]["command"], "mag");
            assert_eq!(
                parsed["mcpServers"]["mag"]["args"],
                serde_json::json!(["serve", "--stdio"])
            );
        });
    }

    // -----------------------------------------------------------------------
    // Remove creates backup
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn remove_creates_backup() {
        with_temp_home(|home| {
            let config_path = home.join(".claude.json");
            let initial = r#"{"mcpServers": {"mag": {"command": "mag", "args": ["serve"]}}}"#;
            std::fs::write(&config_path, initial).expect("write initial");

            let detected = make_detected(AiTool::ClaudeCode, config_path.clone());
            let result = remove_config(&detected).expect("remove should succeed");
            assert_eq!(result, RemoveResult::Removed);

            let bak_path = backup_path_for(&config_path);
            assert!(bak_path.exists());
            let backup_content = std::fs::read_to_string(&bak_path).expect("read backup");
            assert_eq!(backup_content, initial);
        });
    }

    // -----------------------------------------------------------------------
    // Plugin verification tests
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn verify_claude_plugin_returns_false_when_no_settings() {
        with_temp_home(|_home| {
            let result = verify_claude_plugin().expect("verify should not error");
            assert!(!result);
        });
    }

    #[test]
    #[serial]
    fn verify_claude_plugin_returns_false_when_no_enabled_plugins() {
        with_temp_home(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(claude_dir.join("settings.json"), r#"{"theme": "dark"}"#).unwrap();

            let result = verify_claude_plugin().expect("verify should not error");
            assert!(!result);
        });
    }

    #[test]
    #[serial]
    fn verify_claude_plugin_returns_true_when_enabled() {
        with_temp_home(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(
                claude_dir.join("settings.json"),
                r#"{"enabledPlugins": {"mag@mag-plugins": true}}"#,
            )
            .unwrap();

            let result = verify_claude_plugin().expect("verify should not error");
            assert!(result);
        });
    }

    #[test]
    #[serial]
    fn verify_claude_plugin_returns_false_when_disabled() {
        with_temp_home(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(
                claude_dir.join("settings.json"),
                r#"{"enabledPlugins": {"mag@mag-plugins": false}}"#,
            )
            .unwrap();

            let result = verify_claude_plugin().expect("verify should not error");
            assert!(!result);
        });
    }

    #[test]
    #[serial]
    fn verify_claude_plugin_handles_malformed_json() {
        with_temp_home(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            std::fs::write(claude_dir.join("settings.json"), "not valid json{{{").unwrap();

            let result = verify_claude_plugin().expect("verify should not error");
            assert!(!result);
        });
    }

    #[test]
    fn config_write_result_plugin_variant() {
        let result = ConfigWriteResult::Plugin;
        // Verify the variant exists and Debug works
        assert_eq!(format!("{result:?}"), "Plugin");
    }
}
