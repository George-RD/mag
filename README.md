# MAG

Memory-Augmented Generation for local agents and MCP clients.

MAG is a Rust-first memory system for coding agents and MCP-compatible tools. It stores structured memory in SQLite, supports lexical and semantic retrieval, exposes both a CLI and a stdio MCP server, and runs without a Python runtime or a separate vector database.

## Why MAG

- One local binary, one local database.
- Hybrid retrieval with FTS5, ONNX embeddings, optional `sqlite-vec`, graph traversal, and multi-phase advanced search.
- Agent-oriented workflows: checkpoints, reminders, lessons, profile state, lifecycle tools, and MCP integration.
- Portable operations: additive migrations, JSON export/import, and no standing service dependency.

## Compared With omega-memory

MAG and `omega-memory` solve the same class of problem: durable memory for local agents. The difference is operating model.

- MAG is Rust-native and ships as a single local binary.
- MAG uses SQLite directly for storage and retrieval.
- MAG keeps `omega-memory` as a comparison target and lineage reference, not as the product identity.

On the shared local benchmark in this repo, MAG scored higher overall (`98 / 100` vs `90 / 100`). On that same workload, `omega-memory` was faster on both seeding and query latency. The README reflects that tradeoff directly instead of collapsing it into a one-line “winner” claim.

## Quick Start

### Build

```bash
cargo build --release
```

The main binary is `./target/release/mag`.

### Show Active Paths

```bash
cargo run --release -- paths
```

New installs use `~/.mag/`. For one release cycle, if `~/.mag/` does not exist but `~/.romega-memory/` does, MAG continues using the legacy root. The `paths` command shows the active data, database, model, and benchmark-cache locations explicitly.

### Download Models

```bash
cargo run --release -- download-model
cargo run --release -- download-cross-encoder
```

Model files are cached under the active MAG root, usually `~/.mag/models/`.

### Warm Benchmark Datasets

```bash
cargo run --release --bin fetch_benchmark_data -- --dataset all
```

Benchmark datasets are fetched externally and cached under the active MAG root, usually:

- `~/.mag/benchmarks/longmemeval/`
- `~/.mag/benchmarks/locomo/`

### Try the CLI

```bash
cargo run --release -- ingest "The retry logic should use exponential backoff with jitter"
cargo run --release -- search "retry logic"
cargo run --release -- semantic-search "how should retries work?"
cargo run --release -- advanced-search "deployment rollback process"
cargo run --release -- recent --limit 5
cargo run --release -- --help
```

### Run the MCP Server

```bash
cargo run --release -- serve
./target/release/mag serve
```

Copy `.mcp.json.example` to `.mcp.json` and point your MCP client at the local binary.

## Benchmarks

Latest reruns in this branch were taken on `2026-03-12` at commit `66a9e3e97e0e65328864c4699ad6b14ccf8a24ae`, on `macos aarch64, 12 CPU`.

| Benchmark | Result | Notes |
| --- | --- | --- |
| Local LongMemEval-style set | `98 / 100` | `2570 ms` seeding, `2081 ms` querying |
| LoCoMo10 | `476 / 1986` (`24.0%`) | `5882` memories ingested, `252.1 s` total, `22.6 ms` avg query |
| Scale benchmark | `100% Recall@5` at `1K`, `5K`, `10K` | `18.50 ms` mean, `41.85 ms` p95, `49.90 ms` p99 at `10K` |
| `omega-memory` comparison | MAG `98 / 100` vs omega `90 / 100` | MAG seeded faster; omega queried faster on this local workload |
| Official `LongMemEval_S` | Fetch flow implemented; rerun pending | current shell could not resolve the public dataset hosts |

Full methodology, commands, and result tables are in [docs/benchmarks.md](/Users/george/.codex/worktrees/71fd/romega-memory/docs/benchmarks.md).

### Benchmark Safety

Benchmark runs do not touch the normal MAG production database. The official LongMemEval harness uses a fresh in-memory SQLite database per question, and the LoCoMo harness uses a fresh in-memory SQLite database per sample. The main persistent side effect is dataset/model caching under the active MAG root.

## Retrieval Model

MAG currently supports:

- text search over FTS5
- semantic search over ONNX embeddings
- similar-memory lookup from a stored memory ID
- graph traversal and version-chain lookup
- advanced retrieval that fuses vector and lexical candidates

The advanced path combines vector similarity and FTS hits with reciprocal-rank fusion, then refines ranking with event type, time decay, word overlap, importance, priority, and feedback signals.

## Architecture

```text
mag
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── mcp_server.rs
│   ├── app_paths.rs
│   ├── benchmarking.rs
│   └── memory_core/
├── benches/
│   ├── longmemeval/
│   ├── locomo/
│   ├── onnx_profile.rs
│   └── scale_bench.rs
└── tests/
```

## Development

### Local Quality Gate

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Benchmark Commands

```bash
cargo run --release --bin fetch_benchmark_data -- --dataset all
cargo run --release --bin longmemeval_bench -- --json
cargo run --release --bin longmemeval_bench -- --official --questions 10 --json
cargo run --release --bin locomo_bench -- --json
cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50
uv run --project ~/repos/omega-memory python benches/python_comparison.py
```

## License

MIT
