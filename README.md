# MAG

Memory-Augmented Generation for local agents and MCP clients.

`MAG` is the product name. The Rust crate, binary, and default data directory are still named `romega-memory`, so the commands and paths below use that name.

## What MAG Is

MAG is a Rust-first memory system for coding agents and MCP-compatible tools. It stores structured memory in SQLite, supports text and semantic retrieval, exposes a local CLI and stdio MCP server, and runs without a Python runtime or an external vector database.

It is designed for the workflows this repository already supports well:

- local-first agent memory with one binary and one database
- hybrid retrieval with SQLite FTS5 plus optional `sqlite-vec` acceleration
- task continuity features like checkpoints, reminders, lessons, and profile state
- portable export/import without standing infrastructure

## Why Use MAG

- Single Rust binary. No Python environment, no separate vector service, no hosted dependency.
- Hybrid retrieval. Text search, semantic search, phrase search, similar-memory lookup, graph traversal, and multi-phase advanced search.
- Practical agent surface. The current build ships 32 CLI commands and 16 MCP tools.
- Local embeddings. Uses `bge-small-en-v1.5` through ONNX by default.
- SQLite-first operations. WAL mode, additive migrations, FTS5 indexing, and optional `sqlite-vec`.

## Benchmarks

These numbers were rerun on `2026-03-11 22:24:31 +04` on branch `codex/retrieval-scaling-batch` at commit `80a4d7ebc2827ea4b1c7d80e1a76b4dff23ddaa3`, on a MacBook Pro (`Mac14,6`, Apple M2 Max, 12 CPU cores, 32 GB RAM) running macOS `26.3 (25D125)`. Commands used: `cargo run --release --bin longmemeval_bench -- --json` and `cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50`.

### LongMemEval Local Rerun

Command used:

```bash
cargo run --release --bin longmemeval_bench -- --json
```

| Metric | Result |
| --- | --- |
| Dataset | 80 seeded memories, 100 queries, 5 categories |
| Overall accuracy | 98 / 100 (98.0%) |
| Seeding time | 724 ms |
| Query time | 856 ms |
| Peak RSS | 346,032 KB |

| Category | Score |
| --- | --- |
| Information extraction | 20 / 20 |
| Multi-session reasoning | 20 / 20 |
| Temporal reasoning | 19 / 20 |
| Knowledge update | 19 / 20 |
| Abstention | 20 / 20 |

### Scale Benchmark Rerun

Command used:

```bash
cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50
```

| Scale | Mean Search | P95 | P99 | Recall@5 |
| --- | ---: | ---: | ---: | ---: |
| 1K memories | 5.23 ms | 10.18 ms | 10.74 ms | 100.0% |
| 5K memories | 4.81 ms | 9.44 ms | 12.20 ms | 100.0% |
| 10K memories | 13.72 ms | 25.46 ms | 27.00 ms | 100.0% |

From 1K to 10K memories, the rerun showed 2.6x mean-search slowdown and 2.5x P95 slowdown while keeping Recall@5 at 100%.

## Quick Start

### Build

```bash
cargo build --release
```

The binary is written to `./target/release/romega-memory`.

### Download the Embedding Model

```bash
cargo run --release -- download-model
```

This downloads `bge-small-en-v1.5` to `~/.romega-memory/models/`. The model is also auto-downloaded on first use.

### Try the CLI

```bash
# Store a memory
cargo run --release -- ingest "The retry logic should use exponential backoff with jitter"

# Search by text
cargo run --release -- search "retry logic"

# Search by meaning
cargo run --release -- semantic-search "how should retries work?"

# Hybrid retrieval with richer scoring
cargo run --release -- advanced-search "deployment rollback process"

# Explore recent context
cargo run --release -- recent --limit 5

# Full help
cargo run --release -- --help
```

### Run the MCP Server

```bash
cargo run --release -- serve
```

Or run the built binary directly:

```bash
./target/release/romega-memory serve
```

Copy `.mcp.json.example` to `.mcp.json` and point your MCP client at the local binary.

## Current Surface Area

### CLI

The CLI currently exposes 32 commands, including:

- ingest, process, retrieve, delete, update
- list, recent, search, semantic-search, phrase-search, advanced-search, similar
- relations, traverse, version-chain
- checkpoint, resume-task, remind, lessons, profile, welcome, protocol
- maintain, sweep, stats, stats-extended, export, import
- download-model, download-cross-encoder, serve

### MCP Tools

The MCP server currently exposes 16 tools:

| Category | Tools |
| --- | --- |
| Storage | `memory_store`, `memory_store_batch`, `memory_retrieve`, `memory_delete`, `memory_update` |
| Retrieval | `memory_search`, `memory_list`, `memory_relations` |
| Lifecycle | `memory_feedback`, `memory_lifecycle` |
| Cross-session | `memory_checkpoint`, `memory_remind`, `memory_lessons`, `memory_profile` |
| System | `memory_admin`, `memory_session_info` |

## Retrieval Model

MAG supports several retrieval paths:

- FTS5 text search with porter stemming
- semantic search over ONNX embeddings
- similar-memory lookup from an existing memory ID
- relationship traversal and version-chain lookup
- advanced search that fuses vector and lexical candidates

The advanced path combines vector similarity and FTS results, applies reciprocal-rank fusion, boosts dual matches, and then refines the score with factors like event type, time decay, priority, word overlap, importance, and feedback signals.

## Architecture

```text
romega-memory
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── mcp_server.rs
│   └── memory_core/
│       ├── mod.rs
│       ├── embedder.rs
│       ├── reranker.rs
│       ├── scoring.rs
│       └── storage/sqlite/
│           ├── mod.rs
│           ├── schema.rs
│           ├── crud.rs
│           ├── search.rs
│           ├── advanced.rs
│           ├── graph.rs
│           ├── lifecycle.rs
│           ├── session.rs
│           ├── admin.rs
│           └── helpers.rs
├── benches/
│   ├── longmemeval/
│   ├── onnx_profile.rs
│   └── scale_bench.rs
└── tests/
    ├── cli_output_smoke.rs
    ├── longmemeval_regression.rs
    ├── mcp_smoke.rs
    └── parity_harness.rs
```

Core implementation choices:

- SQLite storage with additive schema migrations
- blocking DB work isolated in `tokio::task::spawn_blocking`
- ONNX embeddings behind the `Embedder` trait
- optional `sqlite-vec` acceleration for vector candidate generation
- hot-cache and query-cache support in the SQLite storage layer

## Development

### Local Quality Gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Benchmark Commands

```bash
cargo run --release --bin longmemeval_bench
cargo run --release --bin longmemeval_bench -- --json
cargo run --release --bin longmemeval_bench -- --concurrent
cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50
cargo run --release --bin onnx_profile
```

### Feature Flags

| Flag | Default | Description |
| --- | :---: | --- |
| `real-embeddings` | ON | ONNX runtime, tokenizer support, model download |
| `sqlite-vec` | OFF | Vector acceleration through `sqlite-vec` |
| `mimalloc` | OFF | Alternative allocator |

## Lineage

MAG is implemented in this repository as `romega-memory`, an independent Rust reimplementation inspired by [omega-memory](https://github.com/pchaganti/gx-omega-memory). The conceptual lineage is shared; the code is not.

## License

MIT
