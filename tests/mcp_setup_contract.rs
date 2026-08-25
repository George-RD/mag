#[test]
fn generated_command_transport_prefers_minimal_mcp_surface() {
    let source = include_str!("../src/config_writer.rs");

    assert!(
        source.contains(r#"\"args\": [\"serve\", \"--mcp-tools\", \"minimal\"]"#),
        "new command-transport configs must explicitly request MAG's four-tool minimal MCP surface"
    );
}
