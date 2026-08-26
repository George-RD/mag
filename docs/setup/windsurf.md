# Windsurf Setup
<!-- Last verified: 2026-08-26 | Valid for: v0.1.10+ -->

Add MAG to your Windsurf MCP configuration.

## Config

Edit `.windsurf/mcp.json` in your project root:

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

## Status

Community-reported working. If you run into issues, [open an issue](https://github.com/George-RD/mag/issues).
