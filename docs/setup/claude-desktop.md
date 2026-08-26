# Claude Desktop Setup
<!-- Last verified: 2026-08-26 | Valid for: v0.1.10+ -->

Add MAG to your Claude Desktop MCP configuration.

## Config

Edit `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

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

Replace `/path/to/mag` with the actual path to your MAG binary (e.g., `~/.cargo/bin/mag` on Unix or `C:\Users\<user>\.cargo\bin\mag.exe` on Windows).

The minimal surface exposes MAG's four facade tools. Plain `serve` remains
available only for compatibility with older 19-tool integrations.

## Common Issues

- **"Server disconnected"**: Check that the binary path is correct and the binary is executable (macOS/Linux: `chmod +x`).
- **Model not found on first run**: Run `mag download-model` once before starting Claude Desktop.
