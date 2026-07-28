# Repository Guidelines
<!-- Last verified: 2026-05-20 | Valid for: v0.1.10-dev+ -->

Universal agent guidance for AI coding assistants working on this repository.
Vendor-neutral — applies to Claude Code, Cursor, Windsurf, Copilot, and any AI tool.

---

## Project Overview

**MAG** (`mag-memory`) is a local MCP (Model Context Protocol) memory server written in Rust. It persists memories in SQLite with ONNX embeddings (`bge-small-en-v1.5`, 384-dim by default) for semantic search. No external services are required at runtime.

It exposes **19 MCP tools** via stdio protocol and supports multiple AI coding tool integrations (Claude Code, Cursor, Windsurf, Cline, OpenCode, Claude Desktop) via an interactive `mag setup` wizard.

---

## Architecture & Data Flow

Single binary, single SQLite file, hybrid retrieval.

```
CLI (clap) ──► MCP Server (stdio) ──► Pipeline ──► SQLiteStorage
                                          │
                              ┌───────────┼───────────┐
                              ▼           ▼           ▼
                         Vector KNN   FTS5 BM25   Graph Edges
                              │           │           │
                              └──────► RRF Fusion ◄───┘
                                          │
                              ┌───────────┼───────────┐
                              ▼           ▼           ▼
                         Cross-Encoder  Scoring    Abstention
                         Reranker       Refinement   Gate
```

### Storage Pipeline (store)

1. **Dedup** — SHA-256 canonical hash + cosine similarity check (skip embedding if duplicate)
2. **Embedding** — ONNX embedder (bge-small-en-v1.5 int8, 384-dim); `PlaceholderEmbedder` when `real-embeddings` is off
3. **Supersession** — Mark older similar memories as superseded (cosine >= 0.70)
4. **Relationship linking** — Auto-create `RELATED` edges for near-duplicates
5. **Entity extraction** — Rule-based: people, tools, projects → tags like `entity:people:alice`
6. **Persist** — `memories` table + FTS5 index + `relationships` graph table

### Retrieval Pipeline (advanced search)

1. **Intent classification** — Keyword / Factual / Conceptual / General
2. **Query embedding** — Skip for Keyword queries
3. **Retrieval** — Vector KNN + FTS5 BM25 (parallel)
4. **Fusion** — RRF with dual-match boost
5. **Rerank** — Optional cross-encoder (`ms-marco-MiniLM-L-6-v2`)
6. **Scoring refinement** — `type × time_decay × priority × word_overlap × importance × feedback × query_coverage`
7. **Graph enrichment** — Neighbor boost (factor 0.1) via `relationships` table
8. **Abstention gate** — Dedup + threshold filter + hot-cache merge
9. **Query decomposition** — Multi-topic queries split into sub-queries, merged results

### Key Modules

| Module | Responsibility |
|--------|--------------|
| `src/main.rs` | CLI entry point, clap subcommands, `mag doctor` |
| `src/lib.rs` | Public API surface, feature-gated re-exports |
| `src/memory_core/mod.rs` | `Pipeline` orchestrator, trait re-exports |
| `src/memory_core/domain.rs` | Core types: `EventType`, `MemoryKind`, `MemoryInput`, `SearchOptions`, `SearchResult`, TTL constants |
| `src/memory_core/traits.rs` | 27+ async traits (`Ingestor`, `Storage`, `Searcher`, `AdvancedSearcher`, `GraphTraverser`, `MaintenanceManager`, …) |
| `src/memory_core/embedder.rs` | `OnnxEmbedder` (real) and `PlaceholderEmbedder` (SHA256 fallback) |
| `src/memory_core/scoring.rs` | `ScoringParams`, type weights, word overlap, Jaccard |
| `src/memory_core/storage/sqlite/` | Full SQLite backend: schema, CRUD, search, graph, entities, lifecycle, session, admin, pipeline |
| `src/mcp/mod.rs` | MCP stdio server, `TOOL_REGISTRY`, `#[tool_router]` delegation |
| `src/mcp/tools/` | Tool handlers: `storage.rs`, `search.rs`, `relations.rs`, `lifecycle.rs`, `session.rs`, `facades.rs` |
| `src/setup.rs` | Interactive `mag setup` wizard, connector content installation |
| `src/uninstall.rs` | `mag uninstall` — config removal, cleanup |
| `src/cli.rs` | CLI argument definitions (clap derive) |
| `src/app_paths.rs` | XDG path resolution (`~/.mag/`) |

### Feature Flags

| Flag | Default | Purpose |
|------|---------|---------|
| `real-embeddings` | **ON** | ONNX runtime, tokenizers, model download |
| `mimalloc` | OFF | Alternative allocator |
| `sqlite-vec` | OFF | Vector search acceleration |
| `daemon-http` | OFF | HTTP daemon mode |
| `test-helpers` | OFF | Expose `with_temp_home` to integration tests |

---

## Key Directories

```
src/                             Rust source — core + MCP + CLI
src/memory_core/                 Pipeline, traits, domain types, embedder, scoring
src/memory_core/storage/sqlite/  SQLite backend (schema, CRUD, search, graph, pipeline phases)
src/mcp/                         MCP protocol server and tool handlers
src/mcp/tools/                   Individual tool implementations
tests/                           Integration tests, conformance suite, MCP smoke tests
benches/                         Benchmark binaries (longmemeval, locomo, scale, onnx_profile)
python/                          Python harnesses and MCP client for active strategy research
scripts/                         Shell scripts: bench.sh, smoke-test.sh, bump-version.sh
connectors/                      IDE/agent integration content (cursor, windsurf, opencode, shared)
docs/                            Architecture docs, benchmark logs, MCP tool reference, setup guides
```

---

## Repository Skill Routing

Compatible coding agents commonly load this root `AGENTS.md` automatically. Skill
folder discovery varies by client, so this file explicitly routes environment work:

- Before clean VM/container setup, first-run model debugging, local LLM work, or
  benchmark execution, read `skills/mag-development/SKILL.md`.
- Treat that skill as the operational supplement to this architecture guide; do
  not duplicate the full repository overview into the skill.

---

## Development Commands

### Build
```bash
cargo build --release                          # Release binary
cargo build --bin mag                          # Default binary
cargo build --features real-embeddings         # Explicit default feature
```

### Test
```bash
cargo test --all-features                      # Full test suite (500+ tests)
cargo test --all-features <test_name>          # Single test
cargo test --all-features -- --nocapture       # With output
```

### Lint / Format
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

### Run
```bash
cargo run -- serve                             # Start MCP stdio server
cargo run -- setup                             # Interactive setup wizard
cargo run -- doctor                            # Health check + auto-fix
```

### Benchmarks
```bash
# Preferred entry point (logs to docs/benchmarks/benchmark_log.csv)
./scripts/bench.sh                             # bge-small, 2 samples, word-overlap
./scripts/bench.sh --gate                      # PR gate: compare vs 10-sample baseline
./scripts/bench.sh --samples 10                # Full validation run

# Raw benchmark binaries
cargo run --release --bin longmemeval_bench
cargo run --release --bin locomo_bench -- --samples 2
cargo run --release --bin locomo_bench -- --llm-judge  # needs OPENAI_API_KEY
```

### Quality Gate (all in one)
```bash
prek run                                       # fmt + clippy + test
```

---

## Code Conventions & Common Patterns

### Commits
Semantic commits: `<type>(<scope>): <description>`
Examples: `feat(memory): add TTL sweep`, `fix(search): handle empty FTS5 result`

### Error Handling
- **No `unwrap()` / `expect()` in production** — use `anyhow::Context` or `?`
- All DB I/O in `tokio::task::spawn_blocking` — never block the async executor
- `anyhow::Result<()>` at CLI boundaries; structured errors inside MCP tools map to `McpError`

### Async Patterns
- `#[async_trait]` for trait-based async interfaces
- `tokio::spawn_blocking(|| { conn.call(...) })` for SQLite operations
- `Arc<SqliteStorage>` shared across tool handlers (cloned per request)

### Trait-First Design
- Add new capability via new trait + impl rather than modifying existing signatures
- 27+ traits in `src/memory_core/traits.rs` define the pipeline contract
- `Backend<'a>` struct bundles `&dyn Trait` references for generic test helpers

### Struct-Based API
- `MemoryInput` — store parameters (content, tags, event_type, session_id, …)
- `MemoryUpdate` — update parameters (content, tags, metadata, …)
- `SearchOptions` — search/filter configuration
- Avoid positional arguments in internal APIs

### Naming
- `snake_case` for functions/variables, `PascalCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants
- Module files: `domain.rs`, `traits.rs`, `schema.rs`, `crud.rs`
- Test modules: `mod tests { ... }` inline, or `tests/<name>.rs` for integration

### State Management
- SQLite is the single source of truth; no in-memory caches for durability-critical data
- Selective cache invalidation on `store()`, full clear on bulk ops (import/sweep/compact)
- Connection pooling with reader/writer separation in `conn_pool.rs`

### Schema Migrations
- **Additive only** — never drop/rename columns
- Use `ALTER TABLE ADD COLUMN` with error ignoring for idempotency
- Schema version tracked in `schema_migrations` table

### SQLite Concurrency
- `retry_on_lock()` with bounded backoff: initial + 5 retries, 10-160ms + jitter
- `rusqlite` with `bundled` feature

---

## Important Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Binary entry point, CLI dispatch, doctor checks |
| `src/lib.rs` | Library exports, `#[cfg]` re-exports |
| `Cargo.toml` | Package config, features, dependencies, bin targets, lints |
| `src/memory_core/mod.rs` | `Pipeline` struct, trait re-exports |
| `src/memory_core/traits.rs` | 27+ trait definitions (contract for all backends) |
| `src/memory_core/domain.rs` | Core data types and constants |
| `src/memory_core/embedder.rs` | `OnnxEmbedder` / `PlaceholderEmbedder` |
| `src/memory_core/scoring.rs` | Scoring parameters and algorithms |
| `src/mcp/mod.rs` | MCP server, `TOOL_REGISTRY`, `#[tool_router]` |
| `src/mcp/request_types.rs` | All MCP request/response structs |
| `src/mcp/validation.rs` | `require_finite`, `MAX_RESULT_LIMIT`, `MAX_BATCH_SIZE` |
| `src/setup.rs` | `mag setup` wizard and connector installation |
| `src/uninstall.rs` | `mag uninstall` cleanup |
| `src/app_paths.rs` | XDG paths (`~/.mag/memory.db`, `~/.mag/models/`) |
| `scripts/bench.sh` | Standardized benchmark runner + gate |
| `docs/architecture.md` | Deep-dive on storage/retrieval pipelines |
| `docs/mcp-tools.md` | Full MCP tool reference |
| `docs/configuration.md` | Runtime configuration options |
| `connectors/shared/AGENTS.md` | Template for IDE AGENTS.md injection |

---

## Runtime/Tooling Preferences

### Required Toolchain
- **Rust** (edition 2024) — `cargo` is the only build tool
- **No Node/Bun** required — this is a pure Rust project
- Optional: Python 3 for `python/` benchmark harnesses and active strategy research

### Dependencies
- `tokio` (full) — async runtime
- `rusqlite` (bundled) — SQLite
- `ort` + `tokenizers` + `ndarray` — ONNX embedding (gated by `real-embeddings`)
- `rmcp` — MCP protocol server/client
- `clap` — CLI parsing
- `serde` + `serde_json` — serialization
- `chrono` — date/time handling
- `tracing` + `tracing-subscriber` — structured logging

### Model Files
- ~134 MB of ONNX models auto-download on first use
- Cached under `~/.mag/models/`
- Default: `bge-small-en-v1.5` (int8, 384-dim)
- Optional: `voyage-4-nano` (int8/fp16/fp32, 512/1024/2048-dim Matryoshka)

### Environment
- `.env.local` contains `OPENAI_API_KEY` — in `.gitignore`, loaded by `dotenvy`
- `HOME` / `USERPROFILE` hermetic isolation in MCP smoke tests
- No stdout in MCP server mode — stdout is the protocol channel; logs to stderr via `tracing`

---

## Testing & QA

### Test Infrastructure
- **500+ unit and integration tests**
- All storage tests use **in-memory SQLite** (`:memory:`) for hermeticity
- `serial_test` crate for tests that touch filesystem state
- `tempfile` crate for temporary directories

### Key Test Files
- `tests/storage_conformance.rs` — Generic async conformance suite run against both `SqliteStorage` and `MemoryStorage`
- `tests/mcp_smoke.rs` — End-to-end MCP stdio protocol tests with hermetic HOME/USERPROFILE isolation
- `tests/schema_migration.rs` — Forward/backward migration compatibility
- `tests/cli_output_smoke.rs` — CLI output snapshot tests
- `tests/setup_model_download.rs` — Setup wizard model download logic
- `tests/longmemeval_regression.rs` — Benchmark regression guard
- `tests/python/` — Python-side tests for benchmark runners

### Test Patterns
- `conformance_tests!` macro generates a test module per backend
- `Backend<'a>` trait-object bundle for generic test helpers
- `PlaceholderEmbedder` used when `real-embeddings` is disabled (deterministic SHA256 hashes)
- MCP smoke tests spawn the binary as a child process and speak JSON-RPC over stdio

### Running Tests
```bash
cargo test --all-features                          # Everything
cargo test --all-features mcp_stdio                # MCP smoke tests only
cargo test --all-features sqlite_conformance       # SQLite conformance only
cargo test --all-features memory_conformance       # In-memory backend only
```

### Quality Gates (must pass before pushing)
1. **Format**: `cargo fmt --all -- --check`
2. **Lint**: `cargo clippy --all-targets --all-features -- -D warnings`
3. **Tests**: `cargo test --all-features`
4. **Benchmark gate** (if touching scoring/search/storage): `./scripts/bench.sh --gate`
   - Runs 2-sample, compares against 10-sample baseline
   - Warns at >2pp delta, fails at >5pp regression
5. **Full validation** (before merge if gate warned): `./scripts/bench.sh --samples 10`

Run `prek run` for gates 1-3. Always use `bench.sh` (not raw `cargo run`) so results are logged to `docs/benchmarks/benchmark_log.csv`.

### Post-Implementation Checklist
- [ ] Quality gates pass (`prek run`)
- [ ] Benchmark shows no regression (if applicable); append row to `docs/benchmarks/benchmark_log.csv`
- [ ] New public APIs have tests
- [ ] Code simplification review — check for unnecessary complexity, duplication
- [ ] Update `AGENTS.md` if architecture or conventions changed

---

## Gotchas

- `benches/locomo/` is a modular 10-file benchmark suite, not a single-file bench
- LoCoMo-10 IS the reduced dataset (original had 50 conversations); `--samples 2` is fast iteration mode
- LoCoMo categories: cat 1=single-hop, 2=temporal, 3=multi-hop, 4=open-domain, 5=adversarial
- `conductor/` is a legacy Gemini CLI artifact — not actively maintained
- `GRAPH_NEIGHBOR_FACTOR=0.1` — graph enrichment Phase 5 re-enabled at conservative factor; guarded by `if > 0.0`
- Tags stored as JSON arrays, queried via `json_each()`
- `SearchOptions::default()` used everywhere for search parameter construction
- Changing default `ScoringParams` affects benchmark baselines — always gate-check

---

## Version Control

- Standard git workflow (not Graphite)
- Feature branches: `feat/...`, `fix/...`, `perf/...`, `refactor/...`
- `git switch -c feat/my-feature` then `git push -u origin feat/my-feature`
- Use `gh pr create` for PRs

---

## Tool-Specific Notes

### Claude Code
- Use `/simplify` after completing implementation work to review for quality

### Codex (OpenAI)
<!-- Add Codex-specific guidance here as needed -->

<!-- cairn:agent-guide-begin -->
## Cairn orientation

This project uses cairn to keep its architecture map in sync with code. Read
`.cairn/AGENTS.md` for full orientation, then follow
`.claude/skills/cairn-dev/SKILL.md` for the development loop.
<!-- cairn:agent-guide-end -->
