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
- MAG is benchmarked directly against `omega-memory` on the shared local workload in this repo.

On this shared local workload, MAG scored higher overall while `omega-memory` was faster on seeding and query latency. See the [Benchmarks](#benchmarks) section below for current numbers.

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

New installs use `~/.mag/`. For one release cycle, if `~/.mag/memory.db` is absent but `~/.romega-memory/memory.db` exists, MAG continues using the legacy root, even when `~/.mag/` already contains cache directories. The `paths` command shows the active data, database, model, and benchmark-cache locations explicitly.

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

Current benchmark snapshots were captured on `2026-03-19` at commit `26e51cf3` on `macOS aarch64`. Word-overlap is the primary LoCoMo metric (comparable to AutoMem's published 90.5%).

### LoCoMo (word-overlap scoring, 2 samples, bge-small-en-v1.5)

| Category | Word-Overlap |
| --- | --- |
| **Overall** | **91.1%** |
| Evidence recall | 90.2% |
| Single-hop | 87.6% |
| Temporal | 91.5% |
| Multi-hop | 75.6% |
| Open-domain | 94.0% |
| Adversarial | 90.9% |

788 memories, 304 questions across 2 samples. Avg query: 34 ms, avg embed: 7.5 ms, seed time: 9.7 s.

### Embedding Model Comparison (LoCoMo word-overlap, 2 samples)

| Model | Dim | WO% | EvRec% | 1-Hop | Temporal | Multi-Hop | Open | Adv | AvgEmb | SeedTime |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **bge-small-en-v1.5** *(default)* | 384 | 91.1% | 90.2% | 87.6% | 91.5% | 75.6% | 94.0% | 90.9% | 7.5 ms | 9.7 s |
| voyage-4-nano INT8 | 1024 | 91.8% | 91.3% | 93.5% | 91.5% | 75.6% | 93.3% | 91.6% | 172 ms | 192 s |
| voyage-4-nano INT8 | 512 | 91.3% | 91.6% | 88.8% | 91.5% | 75.6% | 93.7% | 91.6% | 58 ms | 62 s |
| voyage-4-nano FP32 | 1024 | 91.8% | 91.3% | 93.5% | 91.5% | 75.6% | 93.3% | 91.6% | 82 ms | 105 s |

bge-small-en-v1.5 is the default production embedder. voyage-4-nano adds +0.7 pp word-overlap at 23× slower embedding speed.

### Other Benchmarks (earlier snapshot, 2026-03-12)

| Benchmark | Result | Notes |
| --- | --- | --- |
| Local LongMemEval-style set | `98 / 100` | `1538 ms` seeding, `1013 ms` querying, `335568 KB` peak RSS |
| Scale benchmark | `100% Recall@5` at `1K`, `5K`, `10K` | `19.61 ms` mean, `42.56 ms` p95, `51.94 ms` p99 at `10K` |
| `omega-memory` comparison | MAG `98 / 100` vs omega `90 / 100` | omega seeded and queried faster on this local workload |
| Official `LongMemEval_S` sample | `8 / 10` | external dataset fetch works; full `500`-question publication is still pending |

Full methodology, commands, and result tables are in [docs/benchmarks.md](docs/benchmarks.md). Historical runs are tracked in [docs/benchmark_log.csv](docs/benchmark_log.csv).

### Benchmark Safety

Benchmark runs do not touch the normal MAG production database. The official LongMemEval harness uses a fresh in-memory SQLite database per question, and the LoCoMo harness uses a fresh in-memory SQLite database per sample. The main persistent side effect is dataset/model caching under the active MAG root.

## Retrieval Model

MAG currently supports:

- text search over FTS5
- semantic search over ONNX embeddings
- similar-memory lookup from a stored memory ID
- graph traversal and version-chain lookup
- advanced retrieval that fuses vector and lexical candidates

The advanced path combines vector similarity and FTS hits with reciprocal-rank fusion, then refines ranking with event type, time decay, word overlap, importance, priority, and feedback signals. Queries are classified by intent (Keyword / Factual / Conceptual / General) to weight retrieval modes appropriately. Entity extraction runs at ingest time for auto-tagging and graph-edge creation.

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
│       └── storage/sqlite/
│           └── entities.rs
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

The recommended way to run the standard LoCoMo benchmark:

```bash
./scripts/bench.sh
```

For individual benchmarks or the omega-memory comparison, clone `omega-memory` locally first. The comparison script accepts either `--omega-repo` or the `OMEGA_REPO` environment variable.

```bash
cargo run --release --bin fetch_benchmark_data -- --dataset all
cargo run --release --bin longmemeval_bench -- --json
cargo run --release --bin longmemeval_bench -- --official --questions 10 --json
cargo run --release --bin locomo_bench -- --json
cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50
OMEGA_REPO=/path/to/omega-memory uv run --project "$OMEGA_REPO" python benches/python_comparison.py
```

## License

MIT
