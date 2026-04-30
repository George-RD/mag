# AGENTS.md
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

Universal agent guidance for AI coding assistants working on this repository.
Vendor-neutral — applies to Claude Code, Cursor, Windsurf, Copilot, and any AI tool.

## Commands

```bash
# Quality gate (preferred — uses prek.toml)
prek run

# Or manually:
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features

# Run a single test
cargo test --all-features <test_name>

# Build release binary
cargo build --release

# Run MCP server
./target/release/mag serve

# Benchmarks
cargo run --release --bin longmemeval_bench
cargo run --release --bin longmemeval_bench -- --grid-search  # parameter optimization

# LoCoMo benchmark (retrieval quality, 5 categories)
# --samples 2 for fast iteration (~304q, ~15s); full 10-sample only for validation
cargo run --release --bin locomo_bench -- --samples 2                                # substring baseline
cargo run --release --bin locomo_bench -- --samples 2 --scoring-mode word-overlap    # AutoMem-comparable
cargo run --release --bin locomo_bench -- --llm-judge --samples 2                    # LLM judge (needs OPENAI_API_KEY)

# Standardized benchmark runner (logs to docs/benchmarks/benchmark_log.csv, prints comparison table)
./scripts/bench.sh                                 # bge-small, 2 samples, word-overlap
./scripts/bench.sh --model voyage-nano-int8        # voyage-4-nano INT8 @ 1024-dim
./scripts/bench.sh --model bge-small --samples 10  # full validation run
./scripts/bench.sh --model voyage-nano-fp32 --notes "after scoring tweak"  # with notes
./scripts/bench.sh --gate                          # PR gate: run + compare vs 10-sample baseline
./scripts/bench.sh --gate --notes "pre-merge #142" # PR gate with context

# README update checker (suggests edits, does not modify files)
./scripts/check-readme.sh                          # analyze last 3 commits vs README
./scripts/check-readme.sh "new model, score improved to 91%"  # with context hint

# Additional ONNX embedder variants (for model comparison)
./scripts/bench.sh --model granite       # granite-embedding-30m-english
./scripts/bench.sh --model minilm-l6    # all-MiniLM-L6-v2 (fastest)
./scripts/bench.sh --model minilm-l12   # all-MiniLM-L12-v2
./scripts/bench.sh --model e5-small     # e5-small-v2
./scripts/bench.sh --model bge-base     # bge-base-en-v1.5 (768-dim)
./scripts/bench.sh --model nomic        # nomic-embed-text-v1.5 int8
```

## Architecture

Rust MCP memory server — stores memories in SQLite with ONNX embeddings (bge-small-en-v1.5, 384-dim) for semantic search. 19 MCP tools exposed via stdio protocol. No external services required.

### Key modules

- `src/main.rs` — CLI dispatch via clap
- `src/mcp/mod.rs` — MCP stdio server (19 tools), `TOOL_REGISTRY`, thin `#[tool_router]` delegation
  - `request_types.rs` — all MCP request/response structs
  - `validation.rs` — `require_finite`, `MAX_RESULT_LIMIT`, `MAX_BATCH_SIZE`
  - `tools/` — tool handler implementations by concern:
    - `storage.rs` — store/store_batch/retrieve/delete + `memory` facade
    - `search.rs` — memory_search, memory_list
    - `relations.rs` — memory_relations (list/add/traverse/version_chain)
    - `lifecycle.rs` — memory_update, memory_feedback, memory_lifecycle
    - `session.rs` — checkpoint/remind/lessons/profile/session_info
    - `facades.rs` — memory_manage, memory_session, memory_admin unified facades
- `src/memory_core/mod.rs` — 27+ traits defining the pipeline interface
  - `domain.rs` — `EventType`, `MemoryKind`, TTL constants, relationship type constants
  - `traits.rs` — 27+ trait definitions for the pipeline interface
- `src/memory_core/embedder.rs` — `OnnxEmbedder` (real) and `PlaceholderEmbedder` (SHA256 fallback)
- `src/memory_core/scoring.rs` — 26 externalized `ScoringParams`, type weights, word overlap, Jaccard
- `src/memory_core/storage/sqlite/` — SQLite backend:
  - `schema.rs` — table creation, additive migrations
  - `crud.rs` — store/retrieve/update/delete
  - `search.rs` — FTS5 BM25 + vector similarity
  - `advanced.rs` — `impl AdvancedSearcher` orchestrating the 6-phase RRF pipeline; phase implementations live in `pipeline/`
  - `pipeline/` — phase modules: `retrieval` (vector + FTS5), `rerank` (cross-encoder), `fusion` (RRF + dual-match boost), `scoring` (refinement + keyword conversion), `enrichment` (graph + entity tags), `abstention` (dedup + gate + hot-cache merge), `decomp` (single-query runner)
  - `graph.rs` — relationship traversal (BFS, max_hops)
  - `entities.rs` — entity extraction (people, tools, projects) with auto-tagging on store
  - `lifecycle.rs` — TTL, sweep, feedback
  - `session.rs` — checkpoint, profile, session_info
  - `admin/` — health/export/import/stats, backup/restore (split into `backup.rs`, `maintenance.rs`, `stats.rs`, `welcome.rs`)
  - `helpers.rs` — retry logic, intent classification, cache keys, FTS5 query building
  - `nlp.rs` — entity extraction, topic keywords, sub-query generation
  - `query_classifier.rs` — intent classification (Keyword/Factual/Conceptual/General)
  - `temporal.rs` — temporal query expansion (date parsing, relative dates)
  - `conn_pool.rs` — connection pooling with reader/writer separation
  - `embedding_codec.rs` — encode/decode embeddings, dot_product

### Search pipeline

```
Query → Intent classify (Keyword/Factual/Conceptual/General)
      → ONNX embed (skip for Keyword)
      → Vector search + FTS5 BM25
      → RRF fusion → Cross-encoder rerank (optional)
      → Score refinement (type × time_decay × priority × word_overlap × importance × feedback × query_coverage)
      → Graph enrichment (Phase 5, factor=0.1)
      → Abstention gate → Results
```

### Feature flags

- `real-embeddings` (default ON) — ONNX runtime, tokenizers, model download
- `mimalloc` — alternative allocator
- `sqlite-vec` — vector search acceleration

## Conventions

- Semantic commits: `<type>(<scope>): <description>` (e.g., `feat(memory): add TTL sweep`)
- All DB I/O in `tokio::task::spawn_blocking` — never block the async executor
- No `unwrap()`/`expect()` in production — use `anyhow::Context` or `?`
- No stdout in MCP server mode — stdout is protocol channel; logs to stderr via `tracing`
- Schema migrations additive only — never drop/rename columns; `ALTER TABLE ADD COLUMN` with error ignoring
- Trait-first design — add new trait + impl rather than modifying existing signatures
- Struct-based API: `MemoryInput` (store), `MemoryUpdate` (update), `SearchOptions` (search/filter)
- SQLite lock contention: `retry_on_lock()` with bounded backoff (5 attempts, 10-160ms + jitter)
- Cache invalidation: selective for `store()`, full clear for bulk operations (import/sweep/compact)

## Quality Gates

Every code change MUST pass before pushing:

1. **Format**: `cargo fmt --all -- --check`
2. **Lint**: `cargo clippy --all-targets --all-features -- -D warnings`
3. **Tests**: `cargo test --all-features`
4. **Benchmark** (if touching scoring/search/storage): `./scripts/bench.sh --gate` — runs 2-sample, logs to CSV, compares against 10-sample baseline. Warns at >2pp delta, fails at >5pp.
5. **Full validation** (before merge if gate warned): `./scripts/bench.sh --samples 10 --notes "pre-merge validation"` — authoritative 10-sample run.

Run `prek run` for gates 1-3. Always use `bench.sh` (not raw `cargo run`) so results are logged to the CSV.

## Post-Implementation Checklist

- [ ] Quality gates pass (`prek run`)
- [ ] Benchmark shows no regression (if applicable); if changed, append row to `docs/benchmarks/benchmark_log.csv`
- [ ] New public APIs have tests
- [ ] Run code simplification review — check for unnecessary complexity, duplication, missed reuse
- [ ] Update AGENTS.md if architecture or conventions changed

## Version Control (jj)

This repo uses jj (Jujutsu) in colocated mode.

- **Never use bare `git commit`, `git rebase`, `git checkout`** — only `jj` commands. `git status`/`git log` for reads are fine.
- Working copy is always a live commit (`@`). No staging area — every file change auto-amends `@`.
- To "commit": `jj describe -m "msg" && jj new` (or `jj commit -m "msg"`)
- To push: `jj bookmark set <name> -r @- && jj git push --bookmark <name> --allow-new`
- Bookmarks: `feat/...`, `fix/...`, `perf/...`, `refactor/...`
- `jj undo` reverses any operation. `jj op log` is the reflog.

### PR workflow

1. `jj describe -m "feat(scope): description" && jj new`
2. `jj bookmark set feat/my-feature -r @-`
3. `prek run`
4. `jj git push --bookmark feat/my-feature --allow-new`
5. `gh pr create --head feat/my-feature --title "..." --body "..."`

## Testing

- 500+ unit tests + integration tests, all using in-memory SQLite (`:memory:`)
- MCP smoke tests (`tests/mcp_smoke.rs`) use hermetic HOME/USERPROFILE isolation
- Tags stored as JSON arrays, queried via `json_each()`
- `SearchOptions::default()` used everywhere for search parameter construction

## Gotchas

- `benches/locomo/` is a modular 8-file benchmark suite, not a single-file bench
- LoCoMo-10 IS the reduced dataset (original had 50 conversations); `--samples 2` is fast iteration mode
- LoCoMo categories: cat 1=single-hop, 2=temporal, 3=multi-hop, 4=open-domain, 5=adversarial
- `.env.local` contains OPENAI_API_KEY — in .gitignore, loaded by `dotenvy::from_filename(".env.local")`
- `conductor/` is a legacy Gemini CLI artifact — not actively maintained
- `docs/strongholds/` contains planning docs and coordination artifacts
- Model files (~134 MB) auto-download on first use; cached under `~/.mag/models/`
- `GRAPH_NEIGHBOR_FACTOR=0.1` — graph enrichment Phase 5 re-enabled at conservative factor; guarded by `if > 0.0`
- Git hooks do NOT fire under jj — run `prek run` explicitly before pushing
- Benchmark history: `docs/benchmarks/benchmark_log.csv` (16 cols); methodology at `docs/benchmarks/methodology.md`
- voyage-4-nano ONNX: 2048-dim native; use `--embedder-dim` for Matryoshka truncation (512/1024/2048); quant: int8, fp16, fp32, q4

## Tool-Specific Notes

### Claude Code

- The `/jj` skill handles commit/push/PR workflows — use it instead of raw jj commands when available
- Use `/simplify` after completing implementation work to review for quality
- `isolation: "worktree"` on Agent tool creates git worktrees for parallel work; JJ workspaces preferred when available
- Session-start hooks can inject orchestrator mode — see chief-of-staff plugin

### Codex (OpenAI)

<!-- Add Codex-specific guidance here as needed -->
