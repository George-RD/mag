use serde_json::json;

#[test]
fn generated_command_transport_prefers_minimal_mcp_surface() {
    let source = include_str!("../src/config_writer.rs");

    assert!(
        source.contains(r#""args": ["serve", "--mcp-tools", "minimal"]"#),
        "new command-transport configs must explicitly request MAG's four-tool minimal MCP surface"
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
