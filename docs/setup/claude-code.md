# Claude Code Setup

Add MAG to your Claude Code MCP configuration.

## Config

Edit `.claude/settings.json` in your project root (or `~/.claude/settings.json` for global):

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

- **Tools not loading**: Run `/mcp` in Claude Code to check server status.
- **Slow first query**: The ONNX model loads on first use. Subsequent queries are fast.
