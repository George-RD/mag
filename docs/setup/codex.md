# Codex (OpenAI) Setup
<!-- Last verified: 2026-04-02 | Valid for: v0.1.5+ -->

Add MAG to your Codex CLI MCP configuration.

## Automatic Setup

```bash
mag setup
```

This writes the MCP server entry to `~/.codex/config.toml`, enables the hooks feature, and installs lifecycle hooks to `~/.codex/hooks.json`.

## Manual Setup

Edit `~/.codex/config.toml` (or `.codex/config.toml` in your project root):

```toml
[mcp_servers.mag]
command = "mag"
args = ["serve"]
```

### Enable Hooks (optional)

MAG can auto-capture commit messages, error patterns, and session context via Codex hooks. Enable the hooks feature:

```toml
[features]
codex_hooks = true
```

Then run `mag setup` to install the hook scripts, or manually place a `hooks.json` at `~/.codex/hooks.json`.

## AGENTS.md

Codex reads `AGENTS.md` for project-level instructions. MAG's `AGENTS.md` already includes guidance for all AI tools. No additional configuration is needed.

## What Gets Installed

| File | Purpose |
|------|---------|
| `~/.codex/config.toml` | MCP server entry (`[mcp_servers.mag]`) |
| `~/.codex/hooks.json` | Lifecycle hooks (SessionStart, PostToolUse, Stop) |
| `~/.mag/hooks/codex/*.sh` | Hook scripts for session management and auto-capture |

## Hooks

When hooks are enabled, MAG automatically:

- **SessionStart**: Recalls project context and recent memories at session start.
- **PostToolUse (Bash)**: Captures git/jj commit messages and build/test errors as memories.
- **Stop**: Stores a session summary for continuity across sessions.

## Common Issues

- **Tools not appearing**: Restart Codex after editing config.toml.
- **Hooks not firing**: Ensure `[features] codex_hooks = true` is in your config.toml.
- **Permission denied**: Ensure the `mag` binary is executable and on your PATH.
