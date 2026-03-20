# Cursor Setup

Add MAG to your Cursor MCP configuration.

## Config

Edit `.cursor/mcp.json` in your project root (or `~/.cursor/mcp.json` for global):

```json
{
  "mcpServers": {
    "mag": {
      "command": "/path/to/mag",
      "args": ["serve"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

Replace `/path/to/mag` with the actual path to your MAG binary.

## Common Issues

- **Tools not appearing**: Restart Cursor after editing the config file.
- **Permission denied**: Ensure the binary is executable.
