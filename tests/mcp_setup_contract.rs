use std::path::Path;

use serde_json::{Value, json};

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let (_, after_start) = source
        .split_once(start)
        .unwrap_or_else(|| panic!("missing start marker: {start}"));
    let (region, _) = after_start
        .split_once(end)
        .unwrap_or_else(|| panic!("missing end marker: {end}"));
    region
}

fn assert_manifest_prefers_minimal(label: &str, source: &str, server_name: &str) {
    let manifest: Value = serde_json::from_str(source)
        .unwrap_or_else(|error| panic!("invalid {label} manifest: {error}"));
    assert_eq!(
        manifest["mcpServers"][server_name]["args"],
        json!(["serve", "--mcp-tools", "minimal"]),
        "{label} must advertise MAG's four-tool minimal MCP surface"
    );
}

#[test]
fn generated_command_transport_prefers_minimal_mcp_surface() {
    let source = include_str!("../src/config_writer.rs");

    let json_builder = between(
        source,
        "pub fn build_mag_entry(",
        "// ---------------------------------------------------------------------------\n// Claude Code plugin",
    );
    let json_command = between(
        json_builder,
        "TransportMode::Command => {",
        "TransportMode::Stdio => {",
    );
    assert!(
        json_command.contains(r#""args": ["serve", "--mcp-tools", "minimal"]"#),
        "JSON command transport must request MAG's four-tool minimal MCP surface"
    );

    let toml_builder = between(
        source,
        "fn build_mag_toml_block(",
        "/// Returns `(start, end)` byte indices",
    );
    let toml_command = between(
        toml_builder,
        "TransportMode::Command => {",
        "TransportMode::Stdio => {",
    );
    assert!(
        toml_command.contains(r#"args = [\"serve\", \"--mcp-tools\", \"minimal\"]"#),
        "TOML command transport must request MAG's four-tool minimal MCP surface"
    );
}

#[test]
fn mcp_reference_describes_canonical_minimal_facades() {
    let reference = include_str!("../docs/mcp-tools.md");
    for expected_row in [
        "| `memory` | `store`, `store_batch`, `retrieve`, `search`, `delete` |",
        "| `memory_manage` | `update`, `feedback`, `relations`, `lifecycle` |",
        "| `memory_session` | `info`, `checkpoint`, `remind`, `lessons`, `profile` |",
        "| `memory_admin` | `health`, `list`, `export`, `import` |",
    ] {
        assert!(
            reference.contains(expected_row),
            "MCP reference is missing canonical facade row: {expected_row}"
        );
    }
}

#[test]
fn live_claude_plugin_stays_compatible_with_latest_release() {
    let manifest: Value = serde_json::from_str(include_str!("../plugin/.mcp.json"))
        .expect("invalid Claude plugin manifest");
    assert_eq!(
        manifest["mcpServers"]["mag"]["args"],
        json!(["serve"]),
        "the marketplace reads plugin/.mcp.json from main, so it must keep the released full-mode contract until a binary containing facade search is published"
    );
}

#[test]
fn temporary_mcp_patch_workflow_is_not_committed() {
    let workflow =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/agent-apply-mcp-minimal.yml");
    assert!(
        !workflow.exists(),
        "remove the self-modifying PR patch workflow after it has served its purpose"
    );
}

#[test]
fn bundled_development_manifests_prefer_minimal_mcp_surface() {
    for (label, source, server_name) in [
        ("root example", include_str!("../.mcp.json.example"), "mag"),
        (
            "plugin development",
            include_str!("../plugin/dev/mcp.json"),
            "mag-dev",
        ),
    ] {
        assert_manifest_prefers_minimal(label, source, server_name);
    }
}

#[test]
fn active_manual_setup_examples_prefer_minimal_mcp_surface() {
    for (label, source) in [
        ("README", include_str!("../README.md")),
        ("setup guide", include_str!("../docs/SETUP.md")),
        (
            "Claude Code guide",
            include_str!("../docs/setup/claude-code.md"),
        ),
        (
            "Claude Desktop guide",
            include_str!("../docs/setup/claude-desktop.md"),
        ),
        ("Cline guide", include_str!("../docs/setup/cline.md")),
        ("Cursor guide", include_str!("../docs/setup/cursor.md")),
        ("Windsurf guide", include_str!("../docs/setup/windsurf.md")),
        ("npm README", include_str!("../npm/README.md")),
        ("Python README", include_str!("../python/README.md")),
        ("CLI reference", include_str!("../docs/cli-reference.md")),
        ("MCP reference", include_str!("../docs/mcp-tools.md")),
        (
            "setup contract",
            include_str!("../meta/contracts/runtime.setup.md"),
        ),
    ] {
        assert!(
            source.contains("--mcp-tools") && source.contains("minimal"),
            "{label} must show the preferred minimal MCP command"
        );
        assert!(
            !source.contains(r#""args": ["serve"]"#),
            "{label} must not publish a new full-mode command config"
        );
    }
}
