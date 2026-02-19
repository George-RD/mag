# romega-memory

Rust rewrite of omega-memory.

## Local development

### Build and test

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

### Run CLI

```bash
cargo run -- ingest "hello"
cargo run -- retrieve "<memory-id>"
```

## MCP development setup

This repo includes a project-level MCP config at `.mcp.json`.

- Server name: `romega-memory`
- Startup command: `cargo run -- serve`
- Transport: stdio

Use this config in MCP-aware clients that support project `.mcp.json` files.

### Manual server run

```bash
cargo run -- serve
```

### Fast dev loop

1. Keep one terminal running `cargo run -- serve`
2. Use your MCP client to call:
   - `memory_health`
   - `memory_store`
   - `memory_retrieve`
3. Re-run `cargo test` after code changes
