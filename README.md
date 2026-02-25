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
|--------|:-------------------:|:---------------------:|
| Peak RSS | **12 MB** | 47 MB |
| Seed 80 memories | 186 ms | ~350 ms |
| Run 100 queries | 300 ms | ~170 ms |
| Binary size | 36 MB (arm64) | ~200 MB (venv) |
| Startup time | **14 ms** | ~2 s |
> **Note on query times:** Both implementations run 100 queries sequentially on a single thread (Apple M-series). The Rust version performs per-query ONNX embedding inference inline, while the Python version batches embeddings through its runtime. Query throughput is not a bottleneck in practice — real MCP usage issues one query at a time, where both return in <10 ms.

```
Retrieval Quality (LongMemEval Local)
┌─────────────────────────────────┬───────────┬───────────┐
│ Category                        │ Rust      │ Python    │
├─────────────────────────────────┼───────────┼───────────┤
│ Information Extraction            │  80%      │ 100%      │
│ Multi-Session Reasoning           │  35%      │  80%      │
│ Temporal Reasoning                │  80%      │  60%      │
│ Knowledge Update                  │  50%      │  50%      │
│ Abstention                        │ 100%      │ 100%      │
├─────────────────────────────────┼───────────┼───────────┤
│ Overall                           │  69%      │  78%      │
└─────────────────────────────────┴───────────┴───────────┘
```

> Scoring parameters were optimized via grid search across 2,880 combinations. Temporal reasoning beats Python (80% vs 60%). Multi-session reasoning (35%) and knowledge update (50%) are the next improvement targets. LLM-as-judge evaluation shows MS at 45% (partial credit for multi-part answers).

Run the benchmark yourself:
```bash
cargo run --release --bin longmemeval_bench                # table output
cargo run --release --bin longmemeval_bench -- --json       # machine-readable
cargo run --release --bin longmemeval_bench -- -v           # per-question detail
cargo run --release --bin longmemeval_bench -- --llm-judge  # LLM-as-judge (requires OPENAI_API_KEY)
cargo run --release --bin longmemeval_bench -- --grid-search # parameter optimization
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
Advanced search uses a multi-phase pipeline inspired by information retrieval research:

```
Query → Embed (bge-small-en-v1.5) → Vector Search + FTS5 BM25
  │                                      │
  └────── Reciprocal Rank Fusion (RRF) ────┘
                   │
  Score Refinement: type × priority × word_overlap × importance × feedback
                   │
  Abstention Gate (reject if no good match) → Final Ranked Results
```

**Retrieval phases:**
1. **Vector similarity** — cosine distance on ONNX embeddings (bge-small-en-v1.5, 384-dim)
2. **FTS5 BM25** — SQLite full-text search with tokenized matching
3. **RRF fusion** — combines vector and text rankings with equal weighting
4. **Score refinement** — type weighting, priority factors, word overlap + Jaccard similarity, importance boost, feedback signals
5. **Abstention gate** — returns empty if no candidate exceeds a text-overlap threshold (prevents false positives)

**Memory classification:**
- **Semantic memories** (decisions, lessons, preferences) — no time decay; facts don't expire
- **Episodic memories** (session summaries, task completions) — configurable time decay

All 24 scoring parameters are externalized via `ScoringParams` and can be tuned via grid search.

## Development

### Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Test suite

- **312 unit tests** — storage, search, scoring, TTL, dedup, relationships, etc.
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
