# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Quality gate (run before pushing)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features

# Run a single test
cargo test --all-features <test_name>

# Build release binary
cargo build --release

# Run MCP server
./target/release/romega-memory serve

# Benchmarks (requires real-embeddings feature, seeds 80 memories, runs 100 queries)
cargo run --release --bin longmemeval_bench
cargo run --release --bin longmemeval_bench -- --grid-search  # parameter optimization

# Benchmark measurement (wall time, CPU time, RSS) — run 2-3 warm iterations
# macOS: /usr/bin/time -l   Linux: /usr/bin/time -v
/usr/bin/time -l cargo run --release --bin longmemeval_bench -- --json
```

## Architecture

Rust MCP memory server — stores memories in SQLite with ONNX embeddings (bge-small-en-v1.5, 384-dim) for semantic search. 16 MCP tools exposed via stdio protocol. No external services required.

### Key modules

- `src/main.rs` — CLI dispatch via clap
- `src/mcp_server.rs` — MCP stdio server (16 tools)
- `src/memory_core/mod.rs` — 27 traits defining the pipeline interface (Ingestor, Searcher, Embedder, etc.)
- `src/memory_core/embedder.rs` — `OnnxEmbedder` (real) and `PlaceholderEmbedder` (SHA256 fallback, 32-dim)
- `src/memory_core/scoring.rs` — type weights, priority factors, time decay, word overlap, Jaccard similarity; 24 externalized `ScoringParams`
- `src/memory_core/storage/sqlite/` — SQLite backend split into sub-modules:
  - `schema.rs` — table creation, additive migrations
  - `crud.rs` — store/retrieve/update/delete
  - `search.rs` — FTS5 BM25 + vector similarity search
  - `advanced.rs` — multi-phase RRF pipeline (vector + FTS5 + score refinement + abstention gate)
  - `graph.rs` — relationship traversal (BFS, configurable max_hops)
  - `lifecycle.rs` — TTL, sweep, feedback, dedup
  - `session.rs` — `memory_checkpoint`, `memory_profile`, `memory_session_info` (welcome/protocol)
  - `admin.rs` — `memory_admin` (health/export/import/stats), maintenance
  - `helpers.rs` — shared utilities

### Search pipeline

Query → ONNX embed → Vector search + FTS5 BM25 → RRF fusion → Score refinement (type × time_decay × priority × word_overlap × importance × feedback) → Abstention gate → Results

### Feature flags

- `real-embeddings` (default ON) — ONNX runtime, tokenizers, model download
- `mimalloc` — alternative allocator
- `sqlite-vec` — vector search acceleration

## Conventions

- Semantic commits: `feat(scope):`, `fix(scope):`, `perf(scope):`, `refactor(scope):`
- All DB I/O wrapped in `tokio::task::spawn_blocking` — never block the async executor
- No `unwrap()`/`expect()` in production paths
- No stdout output in MCP server mode — stdout is the protocol channel; logs go to stderr via `tracing`
- Schema migrations are additive only — never drop or rename columns; use `ALTER TABLE ADD COLUMN` with error ignoring
- Trait-first design in `memory_core` — add new trait + impl rather than modifying existing signatures
- Struct-based API signatures: `MemoryInput` (store), `MemoryUpdate` (update), `SearchOptions` (search/filter)

## Testing

- 500+ unit tests + integration tests, all using in-memory SQLite (`:memory:`)
- MCP smoke tests (`tests/mcp_smoke.rs`) use hermetic HOME/USERPROFILE isolation — they spawn child processes
- Tags stored as JSON arrays, queried via `json_each()`
- `SearchOptions::default()` used everywhere for search parameter construction

## Gotchas

- The `conductor/` directory contains product planning docs (tracks, specs, style guides) — it's not runtime code
- Model files (~134 MB) auto-download to `~/.romega-memory/models/` on first use; use `cargo run --release -- download-model` to pre-download
- Event types use the `EventType` enum (22 variants + `Unknown(String)`); priority auto-maps via `EventType::default_priority()`
- Store lifecycle includes canonicalized-content dedup and event-type Jaccard dedup thresholds
- CI runs on GitHub Actions (ubuntu-latest): fmt → clippy → test
- Trait methods in `memory_core` used only from bench binaries (e.g. `embed_batch`) need `#[allow(dead_code)]` — the main lib doesn't call them
- Temporal pipeline: `MemoryInput.referenced_date` → `crud.rs` validates via `validate_iso8601()` → stored as `event_at` column; queries filter via `SearchOptions.event_after/event_before` → SQL WHERE + in-memory post-filter; NULL `event_at` falls back to epoch (1970)
- `GRAPH_NEIGHBOR_FACTOR=0.0` (scoring.rs) — graph enrichment Phase 5 in `advanced.rs` is disabled by grid search; guarded by `if > 0.0` check
