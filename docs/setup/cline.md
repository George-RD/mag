# Cline Setup
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

Add MAG to your Cline MCP configuration.

## Config

Edit `.cline/mcp.json` in your project root (or follow Cline's MCP server configuration docs):

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

## Status

Community-reported working. If you run into issues, [open an issue](https://github.com/George-RD/mag/issues).
