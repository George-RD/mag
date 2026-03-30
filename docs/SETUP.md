# MAG Setup Guide

This guide is designed for both humans and AI assistants. If you're an AI tool helping a user set up MAG, follow each step in order.

## Step 1: Install

Pick one method. The shell installer is recommended — it downloads the correct binary for your platform and runs `mag setup` automatically.

```bash
curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh | sh
```

If you prefer a package manager:

| Method | Command |
|---|---|
| Homebrew | `brew install George-RD/mag/mag` |
| npm | `npm install -g mag-memory` |
| Cargo | `cargo install mag-memory` |
| uv | `uv tool install mag-memory` |
| pip | `pip install mag-memory` |

After installing via a package manager, run `mag setup` manually (the shell installer does this automatically).

## Step 2: Configure Your AI Tools

```bash
mag setup
```

This detects installed AI tools, shows their status, and writes the correct MCP config for each one. If you used the shell installer, this already ran.

To reconfigure later or add new tools, run `mag setup` again.

### Manual Configuration (if `mag setup` doesn't support your tool)

MAG is an MCP server. Add it to your tool's MCP config:

**Claude Code:**
```bash
claude mcp add mag -- mag serve
```

**Claude Desktop** (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):
```json
{
  "mcpServers": {
    "mag": {
      "command": "mag",
      "args": ["serve"]
    }
  }
}
```

**Cursor** (`.cursor/mcp.json` in project root or `~/.cursor/mcp.json` for global):
```json
{
  "mcpServers": {
    "mag": {
      "command": "mag",
      "args": ["serve"]
    }
  }
}
```

**Windsurf** (`~/.codeium/windsurf/mcp_config.json`):
```json
{
  "mcpServers": {
    "mag": {
      "command": "mag",
      "args": ["serve"]
    }
  }
}
```

**VS Code / Copilot** (`.vscode/mcp.json` in project root):
```json
{
  "servers": {
    "mag": {
      "command": "mag",
      "args": ["serve"]
    }
  }
}
```

**npx (no install needed)** — use this in any config above instead of the `mag` binary:
```json
{
  "command": "npx",
  "args": ["-y", "mag-memory", "serve"]
}
```

## Step 3: Verify

After configuring, verify MAG is working:

1. Open your AI tool
2. Ask it: "What MCP tools are available?" — it should list MAG's 16 tools (memory_store, recall_memory, etc.)
3. Test a store and recall:
   - "Remember that I prefer dark mode in all my editors"
   - "What are my editor preferences?"

If tools don't appear, restart your AI tool. MCP servers are loaded at startup.

## Step 4: Best Practices

### What to store

Be specific. "Store important things" produces noise. "Store architectural decisions with rationale" produces signal.

**High-value memories:**
- Architectural decisions and why you made them
- Bug fixes with the root cause and error message (so future searches match)
- Project conventions (naming, branching, deployment steps)
- Personal preferences (coding style, tool configs)
- Session handoffs (what you were working on, what's next)

**Do not store:**
- API keys, passwords, tokens, or secrets. MAG stores in plaintext SQLite.
- Ephemeral info that changes hourly (use your tool's built-in context for that)
- Entire files or large code blocks (store the pattern or decision instead)

### Tagging

Tags make recall precise. Use a consistent scheme:

```
project:<name>     — scope to a project
decision           — architectural choices
bugfix             — root cause + fix pairs
preference         — personal/team preferences
handoff            — session continuity
```

Example: `memory_store("Use exponential backoff with jitter for retries", tags: ["project:api-gateway", "decision"])`

### Importance scores

- `0.9` — Critical decisions, security-sensitive choices
- `0.7` — Standard decisions, useful patterns
- `0.5` (default) — General notes, preferences
- `0.3` — Low-priority context, nice-to-have

### System prompt integration

Add these patterns to your AI tool's system prompt or project instructions (e.g., CLAUDE.md) to guide automatic memory usage:

```
When I make an architectural decision, store it with the rationale. Tag with the project name.
When I solve a bug, store the root cause and fix. Include the error message.
At the end of a work session, store what I was working on and what's next. Tag as "handoff".
Before starting work, recall memories for the current project to load context.
```

## Troubleshooting

| Problem | Fix |
|---------|-----|
| Tools not appearing | Restart your AI tool. MCP servers load at startup. |
| Slow first query | Normal. The ONNX embedding model loads on first use (~2s). Subsequent queries are fast. |
| "command not found: mag" | The binary isn't in PATH. Run `which mag` or reinstall with the shell installer. |
| Permission denied | Run `chmod +x $(which mag)` to make the binary executable. |
| Want to uninstall | `mag setup --uninstall` removes configs. Delete the binary and `~/.mag/` directory. |

## More Resources

- [MCP Tools Reference](mcp-tools.md) — all 16 tools MAG exposes
- [What to Store](what-to-store.md) — 10 system prompt patterns
- [Architecture](architecture.md) — how the search pipeline works
- [Security](../SECURITY.md) — data-flow audit
