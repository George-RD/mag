# romega-memory

A high-performance MCP memory server built in Rust. Inspired by [omega-memory](https://github.com/pchaganti/gx-omega-memory), reimplemented from scratch with a focus on speed, low memory footprint, and zero external dependencies.

## Why romega-memory?

- **Single binary** — no Python, no pip, no virtualenv. One `cargo build --release` and you're done.
- **No LLM required** — local ONNX embeddings (bge-small-en-v1.5, 384-dim) for semantic search. Works fully offline.
- **30 MCP tools** — store, search, relate, checkpoint, profile, remind, and more.
- **3.9× lower peak RSS** than the Python equivalent (12 MB vs 47 MB).
- **Sub-second startup** — 14ms cold start from the release binary.

## Performance

Benchmarked using the [LongMemEval](https://arxiv.org/abs/2407.15853)-inspired evaluation (80 memories, 100 queries, 5 categories):

| Metric | romega-memory (Rust) | omega-memory (Python) |
|--------|:-------------------:|:---------------------:|
| Peak RSS | **12 MB** | 47 MB |
| Seed 80 memories | 294 ms | ~350 ms |
| Run 100 queries | 254 ms | ~170 ms |
| Binary size | 36 MB (arm64) | ~200 MB (venv) |
| Startup time | **14 ms** | ~2 s |

> **Note on query times:** Both implementations run 100 queries sequentially on a single thread (Apple M-series). The Rust version performs per-query ONNX embedding inference inline, while the Python version batches embeddings through its runtime. Query throughput is not a bottleneck in practice — real MCP usage issues one query at a time, where both return in <10 ms.

```
Retrieval Quality (LongMemEval Local)
┌────────────────────────────┬────────┬────────┐
│ Category                   │ Rust   │ Python │
├────────────────────────────┼────────┼────────┤
│ Information Extraction     │  80%   │ 100%   │
│ Multi-Session Reasoning    │  35%   │  80%   │
│ Temporal Reasoning         │  80%   │  60%   │
│ Knowledge Update           │  30%   │  50%   │
│ Abstention                 │   0%   │ 100%   │
├────────────────────────────┼────────┼────────┤
│ Overall                    │  45%   │  78%   │
└────────────────────────────┴────────┴────────┘
```

> Retrieval accuracy is actively improving. The Rust implementation already beats Python on temporal reasoning. Abstention and knowledge update scores require score-threshold filtering and feedback weighting — planned for upcoming releases.

Run the benchmark yourself:

```bash
cargo run --release --bin longmemeval_bench          # table output
cargo run --release --bin longmemeval_bench -- --json # machine-readable
cargo run --release --bin longmemeval_bench -- -v     # per-question detail
```

## Quick Start

### Build

```bash
cargo build --release
```

The binary is at `./target/release/romega-memory` (~36 MB, includes the ONNX runtime).

### Download embedding model

```bash
cargo run --release -- download-model
```

Downloads `bge-small-en-v1.5` (~134 MB) to `~/.romega-memory/models/`. Auto-downloads on first use if not present.

### CLI usage

```bash
# Store a memory
cargo run --release -- ingest "Python's GIL prevents true parallel CPU-bound threads"

# Retrieve by ID
cargo run --release -- retrieve "<memory-id>"

# Search
cargo run --release -- search "Python threading"

# Semantic search (uses embeddings)
cargo run --release -- semantic-search "concurrency in Python"

# Advanced search (hybrid vector + FTS5 with RRF fusion)
cargo run --release -- advanced-search "how to handle shared state"

# List recent memories
cargo run --release -- recent

# Full CLI help
cargo run --release -- --help
```

### MCP Server

```bash
# Start the MCP stdio server
cargo run --release -- serve
```

Or use the pre-built binary for fastest startup:

```bash
./target/release/romega-memory serve
```

The project includes `.mcp.json` for automatic MCP client integration.

## Architecture

```
romega-memory
├── src/
│   ├── main.rs              # CLI dispatch (30 commands)
│   ├── cli.rs               # Clap command definitions
│   ├── mcp_server.rs        # MCP stdio server (30 tools)
│   └── memory_core/
│       ├── mod.rs            # 26 traits + Pipeline orchestration
│       ├── embedder.rs       # ONNX embedder (bge-small-en-v1.5, 384-dim)
│       ├── scoring.rs        # Type weights, priority, time decay, Jaccard
│       └── storage/
│           └── sqlite.rs     # SQLite backend (~7600 lines)
├── benches/
│   └── longmemeval.rs        # LongMemEval benchmark binary
└── tests/
    ├── mcp_smoke.rs          # MCP protocol integration test
    └── parity_harness.rs     # Cross-implementation parity test
```

### MCP Tools (30)

| Category | Tools |
|----------|-------|
| **Core** | `memory_store`, `memory_retrieve`, `memory_delete`, `memory_update` |
| **Search** | `memory_search`, `memory_semantic_search`, `memory_advanced_search`, `memory_tag_search`, `memory_phrase_search`, `memory_similar` |
| **Browse** | `memory_list`, `memory_recent`, `memory_relations`, `memory_traverse` |
| **Lifecycle** | `memory_feedback`, `memory_sweep`, `memory_maintain` |
| **Session** | `memory_checkpoint`, `memory_resume_task`, `memory_profile`, `memory_welcome`, `memory_protocol` |
| **Admin** | `memory_health`, `memory_stats`, `memory_stats_extended`, `memory_export`, `memory_import`, `memory_remind`, `memory_lessons`, `memory_add_relation` |

### Search Pipeline

Advanced search uses **Reciprocal Rank Fusion (RRF)** to combine:

1. **Vector similarity** — cosine distance on ONNX embeddings (bge-small-en-v1.5)
2. **FTS5 BM25** — SQLite full-text search with tokenized matching
3. **Type weighting** — event types (`decision`, `lesson_learned`, etc.) have scoring multipliers
4. **Priority factors** — higher priority memories get score boosts
5. **Time decay** — recent memories rank higher
6. **Word overlap + Jaccard** — lexical similarity bonuses
7. **Importance boost** — user-assigned importance (0.0–1.0)
8. **Context tag matching** — optional tag-based relevance boost

## Development

### Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Test suite

- **172 unit tests** — storage, search, scoring, TTL, dedup, relationships, etc.
- **3 integration tests** — MCP protocol smoke test, parity harness
- All tests use in-memory SQLite (fast, hermetic, no cleanup)

### Conventions

- Semantic commits: `feat(scope): description`
- All DB I/O in `tokio::task::spawn_blocking`
- No `unwrap()`/`expect()` in production paths
- No stdout logging in MCP server mode (protocol corruption)
- Additive schema migrations only (never drop/rename columns)

## Lineage

romega-memory is an independent Rust reimplementation inspired by [omega-memory](https://github.com/pchaganti/gx-omega-memory). The projects share conceptual design (MCP memory server with embeddings) but no code. romega-memory is built from scratch using Rust's type system, async runtime, and SQLite backend.

## License

MIT
