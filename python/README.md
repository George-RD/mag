# mag-memory

PyPI wrapper for [mag](https://github.com/George-RD/mag), a Rust-based MCP memory server.

mag stores memories in SQLite with ONNX embeddings for semantic search, exposing 19 MCP tools via stdio protocol. No external services required.

## Installation

```bash
pip install mag-memory
```

## Usage

```bash
# Start the MCP server
mag serve

# The native binary is downloaded automatically on first run.
# All CLI arguments are passed through to the Rust binary.
```

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
