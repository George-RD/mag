use serde_json::json;

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let (_, after_start) = source
        .split_once(start)
        .unwrap_or_else(|| panic!("missing start marker: {start}"));
    let (region, _) = after_start
        .split_once(end)
        .unwrap_or_else(|| panic!("missing end marker: {end}"));
    region
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
fn claude_plugin_manifest_prefers_minimal_mcp_surface() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../plugin/.mcp.json")).expect("valid plugin manifest");

    assert_eq!(
        manifest["mcpServers"]["mag"]["args"],
        json!(["serve", "--mcp-tools", "minimal"])
    );
}
