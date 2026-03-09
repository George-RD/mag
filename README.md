# romega-memory

A high-performance MCP memory server built in Rust. Inspired by [omega-memory](https://github.com/pchaganti/gx-omega-memory), reimplemented from scratch with a focus on speed, low memory footprint, and zero external dependencies.

## Why romega-memory?

- **Single binary** — no Python, no pip, no virtualenv. One `cargo build --release` and you're done.
- **No LLM required** — local ONNX embeddings (bge-small-en-v1.5, 384-dim) for semantic search. Works fully offline.
- **31 MCP tools** — store, search, relate, checkpoint, profile, remind, version chain, and more.
- **Self-contained** — one binary, one SQLite database, one auto-downloaded model. No external services.
- **Sub-second startup** — 14ms cold start from the release binary.

## Performance

Benchmarked using the [LongMemEval](https://arxiv.org/abs/2407.15853)-inspired evaluation (80 memories, 100 queries, 5 categories). Both implementations measured on the same hardware (Apple M-series), same benchmark data, same ONNX model:

| Metric | Rust | Python |
| --- | :---: | :---: |
| Wall time | 2.0 s | 1.09 s |
| CPU time | 3.0 s | 4.52 s |
| Peak RSS | 327 MB | 326 MB |
| Concurrent QPS (N=16) | **1,665** | N/A |
| Binary size | 36 MB (arm64) | ~200 MB (venv) |
| Startup time | **14 ms** | ~2 s |

> **Note:** Wall time includes seeding 80 memories + running 100 queries on an in-memory SQLite connection. Python uses more CPU time (ONNX parallelizes across all cores) but finishes faster wall-clock because Rust's search pipeline is currently single-threaded per query. Peak RSS includes the ONNX runtime and bge-small-en-v1.5 model. Measured via `/usr/bin/time -l` on macOS.

```text
Retrieval Quality (LongMemEval Local)
┌─────────────────────────┬───────┬────────┐
│ Category                │ Rust  │ Python │
├─────────────────────────┼───────┼────────┤
│ Information Extraction  │ 100%  │  100%  │
│ Multi-Session Reasoning │  90%  │  100%  │
│ Temporal Reasoning      │  75%  │   80%  │
│ Knowledge Update        │  95%  │   95%  │
│ Abstention              │ 100%  │   75%  │
├─────────────────────────┼───────┼────────┤
│ Overall                 │  92%  │   90%  │
└─────────────────────────┴───────┴────────┘
```

> Benchmark uses real ONNX embeddings (bge-small-en-v1.5) with porter stemming, bigram expansion, and dual-match boost. Scoring parameters optimized via grid search across 2,880 combinations. Python numbers from head-to-head comparison using identical benchmark data on the same machine.

Run the benchmark yourself:

```bash
cargo run --release --bin longmemeval_bench                 # table output
cargo run --release --bin longmemeval_bench -- --json        # machine-readable
cargo run --release --bin longmemeval_bench -- --verbose     # per-question detail
cargo run --release --bin longmemeval_bench -- --llm-judge   # LLM-as-judge (requires OPENAI_API_KEY)
cargo run --release --bin longmemeval_bench -- --grid-search # parameter optimization
cargo run --release --bin longmemeval_bench -- --concurrent  # + concurrent throughput (QPS, p50/p95/p99)
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

Copy `.mcp.json.example` to `.mcp.json` (gitignored) and configure it for your local MCP client integration.

## Architecture

```text
romega-memory
├── src/
│   ├── main.rs                  # CLI dispatch (31 commands)
│   ├── cli.rs                   # Clap command definitions
│   ├── mcp_server.rs            # MCP stdio server (31 tools)
│   └── memory_core/
│       ├── mod.rs               # Traits, types, EventType enum, pipeline
│       ├── embedder.rs          # ONNX embedder with batch inference + LRU cache
│       ├── scoring.rs           # Type weights, priority, time decay, stemming, Jaccard
│       └── storage/sqlite/
│           ├── mod.rs           # Connection pool (writer mutex + reader pool)
│           ├── schema.rs        # Table creation, additive migrations
│           ├── crud.rs          # Store/retrieve/update/delete
│           ├── search.rs        # FTS5 BM25 + vector similarity
│           ├── advanced.rs      # Multi-phase RRF pipeline + explainability
│           ├── graph.rs         # Relationship traversal (BFS, max_hops)
│           ├── lifecycle.rs     # TTL, sweep, feedback, dedup
│           ├── session.rs       # Checkpoint, profile, welcome, protocol
│           ├── admin.rs         # Stats, export/import, health, auto-compaction
│           └── helpers.rs       # Shared utilities, FTS5 query builder
├── benches/
│   ├── longmemeval/             # LongMemEval benchmark suite
│   └── scale_bench.rs           # Scale degradation benchmark (1K–50K)
└── tests/
    ├── mcp_smoke.rs             # MCP protocol integration test
    └── parity_harness.rs        # Cross-implementation parity test
```

### Key Design Decisions

- **EventType enum** — 22 typed variants + `Unknown(String)` for forward compatibility. Eliminates string comparisons in hot paths.
- **Connection pool** — Single writer `Mutex` + N reader pool via WAL mode. No `rusqlite::Connection` sharing across threads.
- **Batch embeddings** — `embed_batch()` pads inputs into a single ONNX tensor call with LRU cache deduplication.
- **Zero-copy scoring** — `Cow<str>` in suffix stemming avoids allocations when no stemming applies.

### MCP Tools (31)

| Category | Tools |
| --- | --- |
| **Core** | `memory_store`, `memory_retrieve`, `memory_delete`, `memory_update` |
| **Search** | `memory_search`, `memory_semantic_search`, `memory_advanced_search`, `memory_tag_search`, `memory_phrase_search`, `memory_similar` |
| **Browse** | `memory_list`, `memory_recent`, `memory_relations`, `memory_traverse`, `memory_version_chain` |
| **Lifecycle** | `memory_feedback`, `memory_sweep`, `memory_maintain` |
| **Session** | `memory_checkpoint`, `memory_resume_task`, `memory_profile`, `memory_welcome`, `memory_protocol` |
| **Admin** | `memory_health`, `memory_stats`, `memory_stats_extended`, `memory_export`, `memory_import`, `memory_remind`, `memory_lessons`, `memory_add_relation` |

### Search Pipeline

Advanced search uses a multi-phase pipeline inspired by information retrieval research:

```text
Query → Embed (bge-small-en-v1.5) → Vector Search + FTS5 BM25 (porter stemming)
  │                                         │
  └──────── Reciprocal Rank Fusion (RRF) ────┘
                      │
     Dual-match boost (candidates in both vec + FTS)
                      │
     Score Refinement: type × time_decay × priority × word_overlap × importance × feedback
                      │
     Abstention Gate (reject if no good match) → Final Ranked Results
```

**Retrieval phases:**

1. **Vector similarity** — cosine distance on ONNX embeddings (bge-small-en-v1.5, 384-dim)
2. **FTS5 BM25** — SQLite full-text search with porter stemming and bigram expansion
3. **RRF fusion** — combines vector and text rankings with tuned vector/FTS weights
4. **Dual-match boost** — boosts candidates present in both vector and FTS results
5. **Score refinement** — type weighting, time decay, priority factors, word overlap + Jaccard similarity, importance boost, feedback signals
6. **Abstention gate** — returns empty if no candidate exceeds a text-overlap threshold (prevents false positives)

**Search features:**

- **Explainability** — pass `explain: true` to get component scores (`_explain` metadata) for each result
- **Confidence signal** — results include `confidence` (0.0–1.0) and `abstained` flag
- **Temporal filtering** — `event_after` / `event_before` on `SearchOptions` to filter by time range
- **Temporal expansion** — queries like "last week" or "yesterday" auto-expand to date filters

**Memory classification:**

- **Semantic memories** (decisions, lessons, preferences) — no time decay; facts don't expire
- **Episodic memories** (session summaries, task completions) — configurable time decay

All 24 scoring parameters are externalized via `ScoringParams` and can be tuned via grid search.

### Auto-Compaction

Automatic memory deduplication using embedding similarity:

- Groups similar memories by cosine distance (configurable threshold, default 0.92)
- Uses Union-Find clustering to merge groups transitively
- Keeps the longest memory in each cluster, deletes the rest
- Runs per event type with configurable limits
- Available as `memory_maintain` MCP tool or CLI command

## Development

### Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Test suite

- **500+ unit tests** — storage, search, scoring, TTL, dedup, relationships, versioning, temporal, explainability, compaction, scale
- **Integration tests** — MCP protocol smoke test, parity harness, cross-project isolation
- All tests use in-memory SQLite (fast, hermetic, no cleanup)

### Feature flags

| Flag | Default | Description |
| --- | :---: | --- |
| `real-embeddings` | ON | ONNX runtime, tokenizers, model download |
| `mimalloc` | OFF | Alternative memory allocator |
| `sqlite-vec` | OFF | Vector search acceleration via sqlite-vec |

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
