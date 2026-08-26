# mag-memory

PyPI wrapper for [mag](https://github.com/George-RD/mag), a Rust-based MCP memory server.

mag stores memories in SQLite with ONNX embeddings for semantic search. New MCP integrations use four facade tools; a full 19-tool mode remains for compatibility. No external services required.

## Installation

```bash
pip install mag-memory
```

## Usage

```bash
# Start the preferred four-tool MCP server
mag serve --mcp-tools minimal

# The native binary is downloaded automatically on first run.
# All CLI arguments are passed through to the Rust binary.
```

Plain `mag serve` retains the full 19-tool compatibility mode for existing
integrations.

## How it works

This package does not bundle the native binary. On first run, it detects your platform (Linux/macOS/Windows, x86_64/ARM64), downloads the correct prebuilt binary from [GitHub Releases](https://github.com/George-RD/mag/releases), and caches it locally. Subsequent runs use the cached binary with zero overhead (Unix `exec`).

## Supported platforms

| OS      | Architecture |
|---------|-------------|
| Linux   | x86_64, aarch64 |
| macOS   | x86_64, Apple Silicon (aarch64) |
| Windows | x86_64 |

## License

MIT
