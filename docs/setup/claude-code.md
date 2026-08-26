# Claude Code Setup
<!-- Last verified: 2026-08-26 | Valid for: v0.1.10+ -->

Add MAG to your Claude Code MCP configuration.

## Config

Edit `.claude/settings.json` in your project root (or `~/.claude/settings.json` for global):

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

- **Tools not loading**: Run `/mcp` in Claude Code to check server status.
- **Slow first query**: The ONNX model loads on first use. Subsequent queries are fast.
