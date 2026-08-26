# Cursor Setup
<!-- Last verified: 2026-08-26 | Valid for: v0.1.10+ -->

Add MAG to your Cursor MCP configuration.

## Config

Edit `.cursor/mcp.json` in your project root (or `~/.cursor/mcp.json` for global):

```json
{
  "mcpServers": {
    "mag": {
      "command": "/path/to/mag",
      "args": ["serve", "--mcp-tools", "minimal"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

Replace `/path/to/mag` with the actual path to your MAG binary.

The minimal surface exposes MAG's four facade tools. Plain `serve` remains
available only for compatibility with older 19-tool integrations.

## Common Issues

- **Tools not appearing**: Restart Cursor after editing the config file.
- **Permission denied**: Ensure the binary is executable.
