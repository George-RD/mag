<div align="center">
  <h1>MAG</h1>
  <p><em>Open any tool. Your context is already there.</em></p>
  <p>
    <a href="https://github.com/George-RD/mag/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/George-RD/mag/ci.yml?branch=main" alt="CI"></a>
    <a href="https://crates.io/crates/mag-memory"><img src="https://img.shields.io/crates/v/mag-memory" alt="crates.io"></a>
    <a href="https://www.npmjs.com/package/mag-memory"><img src="https://img.shields.io/npm/v/mag-memory" alt="npm"></a>
    <a href="https://github.com/George-RD/mag/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License"></a>
    <a href="https://github.com/George-RD/mag"><img src="https://img.shields.io/github/stars/George-RD/mag" alt="GitHub Stars"></a>
  </p>
  <p>
    <a href="https://github.com/George-RD/mag/tree/main/docs">Documentation</a> |
    <a href="https://github.com/George-RD/mag/issues">Issues</a> |
    <a href="https://github.com/George-RD/mag/blob/main/docs/benchmarks/methodology.md">Benchmarks</a> |
    <a href="https://github.com/George-RD/mag/blob/main/SECURITY.md">Security</a>
  </p>
</div>

---

Every new session, your AI starts from zero. The decisions you made yesterday, the patterns you taught it, the bugs you already solved together - gone. You re-explain. It re-discovers. You lose hours every week to an assistant with no long-term memory.

MAG fixes this. It gives your AI tools persistent memory that survives across sessions, across projects, and across tools. Open Claude on Monday. It already knows what you decided on Friday.

---

## Quick Start

```bash
curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh | sh
```

```bash
mag ingest "The retry logic should use exponential backoff with jitter"
mag search "how should retries work?"
# → "The retry logic should use exponential backoff with jitter" (score: 0.94)
```

That's it. One command to install, one to store, one to recall. The installer auto-detects your AI tools and configures them — no manual JSON editing needed.

---

## Why Developers Choose MAG

- **Switch from Claude to Cursor. Your context came with you.** One memory store works across every MCP-compatible tool on your machine.
- **Finds what you stored, even when you search differently than how you saved it.** Hybrid retrieval fuses full-text, semantic, and graph search so you don't need exact wording.
- **No accounts. No intermediaries. No "free tier."** A single Rust binary, a single SQLite file. Install it, use it, own it.
- **Your client signed an NDA with you. Not with Mem0's infrastructure.** Zero third-party data routing by default. Your memory data never touches servers you don't control.
- **Your memory, not your vendor's.** Claude, Cursor, and ChatGPT are all building their own memory - but it stays inside their tool. MAG bridges all of them. One memory store, every tool, portable forever.

---

## Works With

| Tool | macOS | Linux | Auto-configured |
|---|---|---|---|
| Claude Code | ✅ | ✅ | `mag setup` |
| Claude Desktop | ✅ | ✅ | `mag setup` |
| Cursor | ✅ | ✅ | `mag setup` |
| VS Code + Copilot | ✅ | ✅ | `mag setup` |
| Windsurf | ✅ | ✅ | `mag setup` |
| Cline | ✅ | ✅ | `mag setup` |
| Gemini CLI | ✅ | ✅ | `mag setup` |
| Zed | ✅ | ✅ | Manual |
| Codex (OpenAI) | ✅ | ✅ | Manual |

Any tool that supports MCP can connect to MAG. Windows is untested - [report your results](https://github.com/George-RD/mag/issues).

---

## Benchmarks

**91.1% retrieval accuracy on the LoCoMo memory benchmark.** Don't trust our number - [run it yourself](docs/benchmarks/methodology.md#running-the-benchmark):

```bash
./scripts/bench.sh
```

AutoMem's published score on the same benchmark is 90.5%. Full methodology, model comparisons, and historical runs in [docs/benchmarks/](docs/benchmarks/).

---

## Your Data, Your Control

Your memory data never touches third-party servers. Not ours, not anyone else's. Same guarantee whether you run MAG on your laptop or deploy it on your own infrastructure.

- Zero third-party data routing (API embedding models are optional, off by default)
- Single SQLite file, portable and inspectable
- Export everything with one command. Open it with any SQLite browser.
- MIT licensed, no tiers, no vendor lock-in
- The binary keeps working whether we maintain this project or not

See [SECURITY.md](SECURITY.md) for the full data-flow audit.

---

## Deploy Your Way

| Mode | Description |
|---|---|
| **Local** (default) | Single binary on your machine. Zero config. |
| **Self-hosted** | Deploy on your own server or cloud. Same privacy guarantees at scale. |
| **MAG Cloud** | Coming soon. We run the infrastructure. You own the data. Same guarantees. |

Every mode: zero third-party data access, full data portability, MIT licensed.

---

## Install

| Method | Command |
|---|---|
| **Shell** (macOS / Linux) | `curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh \| sh` |
| **Homebrew** | `brew install George-RD/mag/mag` |
| **npm** | `npm install -g mag-memory` |
| **uv** | `uv tool install mag-memory` |
| **pip** | `pip install mag-memory` |
| **Cargo** | `cargo install mag-memory` |

**From source:** `git clone https://github.com/George-RD/mag.git && cd mag && cargo build --release`

**Prebuilt binaries:** macOS (x64, ARM), Linux (x64, ARM), Windows (x64) on the [Releases page](https://github.com/George-RD/mag/releases).

---

## Configure Your AI Tools

The installer runs `mag setup` automatically. To reconfigure at any time:

```bash
mag setup
```

This detects installed AI tools, shows their configuration status, and writes the correct MCP config for each one. Use `--non-interactive` for CI or scripted environments.

<details>
<summary>Manual configuration</summary>

MAG runs as an MCP server. Add it to your client's config file:

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

**Claude Code:** `claude mcp add mag -- mag serve`

**npx (no install):**

```json
{
  "mcpServers": {
    "mag": {
      "command": "npx",
      "args": ["-y", "mag-memory", "serve"]
    }
  }
}
```

Per-tool setup guides: [Claude Desktop](docs/setup/claude-desktop.md) | [Cursor](docs/setup/cursor.md) | [Claude Code](docs/setup/claude-code.md) | [Windsurf](docs/setup/windsurf.md) | [Cline](docs/setup/cline.md)

</details>

---

## Learn More

- [Benchmarks](docs/benchmarks/) - full results, model comparisons, methodology
- [Security](SECURITY.md) - data-flow audit, threat model
- [What to Store](docs/what-to-store.md) - get the most out of persistent memory
- [Setup Guides](docs/setup/) - per-tool configuration instructions
- [AGENTS.md](AGENTS.md) - architecture, conventions, development commands

---

## License

MIT
