# MAG Setup Guide

MAG's CLI is the canonical interface. Use it for normal storage, retrieval,
maintenance, and debugging. Configure MCP only when an AI host specifically
needs MCP transport.

## 1. Install

Choose one method:

| Method | Command |
|---|---|
| Shell | `curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh | sh` |
| Homebrew | `brew install George-RD/mag/mag` |
| npm | `npm install -g mag-memory` |
| Cargo | `cargo install mag-memory` |
| uv | `uv tool install mag-memory` |
| pip | `pip install mag-memory` |

The shell installer currently runs `mag setup` after installing, which can
configure detected MCP clients and download local model artifacts. Package-manager
installs only install MAG; run `mag setup` later if you want host integration.

## 2. Verify the CLI first

Run a real store/search cycle before debugging any host integration:

```bash
mag doctor || true
mag ingest "Use exponential backoff with jitter for retries" \
  --tags "project:api-gateway,decision" --importance 0.8
mag advanced-search "How should retries work?" --explain
mag doctor
```

The first `doctor` may report missing model artifacts. A real ingest or search can
download the embedding model on first use, so rerun `doctor` afterwards before
concluding the runtime is broken.

If these commands work, MAG's local runtime and SQLite store are working. MCP is
not required for this path.

## 3. Optional: connect an AI host through MCP

Only do this when the consuming host needs MCP transport.

```bash
mag setup
```

`mag setup` detects supported AI tools and writes command-transport configuration
that launches `mag serve`. The MCP adapter uses the same application/runtime
workflows as the CLI; it is not a separate memory implementation.

To reconfigure later, run `mag setup` again.

### Manual MCP configuration

If `mag setup` does not support your host, configure it to launch `mag serve` over
stdio.

**Claude Code:**

```bash
claude mcp add mag -- mag serve
```

**Hosts using an `mcpServers` object** (for example Claude Desktop, Cursor, and
Windsurf):

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

**VS Code / Copilot** (`.vscode/mcp.json`):

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

If you want the npm package to supply the executable instead of a global `mag`
binary, use:

```json
{
  "command": "npx",
  "args": ["-y", "mag-memory", "serve"]
}
```

`mag serve` is an MCP **stdio** transport. It is not a background HTTP daemon.
The host must keep the child process alive and communicate over stdin/stdout.

## 4. Verify MCP only when you configured it

First confirm the CLI still works. Then restart the AI host and verify that MAG's
facade tools are advertised and can store/search the same data. If MCP fails while
the CLI succeeds, debug host configuration or stdio transport rather than the
memory runtime.

The generated MCP contract and compatibility-tool details live in
[MCP Tools Reference](mcp-tools.md).

## 5. Store useful memory

Be specific. "Store important things" produces noise. "Store architectural
decisions with rationale" produces signal.

High-value examples include:

- architectural decisions and their rationale;
- bug fixes with the root cause and exact error text;
- project conventions such as naming, branching, and deployment rules;
- stable preferences and constraints;
- session handoffs: what was done, what remains, and why.

Do not store API keys, passwords, tokens, or other secrets. MAG stores memories in
plaintext SQLite.

Use consistent tags where they add retrieval value:

```bash
mag ingest "Deploys require manual approval in staging" \
  --tags "project:api-gateway,decision,deploy" --importance 0.9
```

For prompt-driven workflows, describe the behavior rather than coupling prompts to
a transport-specific tool name:

```text
When I make an architectural decision, store it with the rationale and project tag.
When I solve a bug, store the root cause, fix, and exact error message.
At the end of a work session, store what was done and what is next as a handoff.
Before starting work, search memory for the current project and task.
```

If the host is connected through MCP, it can adapt these intentions to MAG's MCP
facade. Otherwise use the CLI commands directly.

## Troubleshooting

| Problem | Check |
|---|---|
| CLI command fails | Run `mag doctor`; isolate `MAG_DATA_ROOT` if testing. |
| Slow first query | Separate first-use model download/warmup from steady-state latency. |
| AI host cannot see MAG | Verify the CLI first, then restart the host and inspect its MCP config. |
| `mag serve` appears to exit | Confirm the MCP client keeps stdin open. |
| `command not found: mag` | Run `which mag`; restart the shell or fix `PATH`. |
| Permission denied | Run `chmod +x "$(which mag)"`. |
| Want to remove host configs | Run `mag setup --uninstall`; delete the binary/data separately if required. |

## More resources

- [CLI Reference](cli-reference.md)
- [What to Store](what-to-store.md)
- [MCP Tools Reference](mcp-tools.md) — optional transport contract
- [Architecture](architecture.md)
- [Security](../SECURITY.md)
