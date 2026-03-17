# AGENTS.md

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
```

## Architecture

Rust MCP memory server — stores memories in SQLite with ONNX embeddings (bge-small-en-v1.5, 384-dim) for semantic search. 16 MCP tools exposed via stdio protocol. No external services required.

### Key modules

- `src/main.rs` — CLI dispatch via clap
- `src/mcp_server.rs` — MCP stdio server (16 tools), `TOOL_REGISTRY` const array
- `src/memory_core/mod.rs` — 27+ traits defining the pipeline interface
- `src/memory_core/embedder.rs` — `OnnxEmbedder` (real) and `PlaceholderEmbedder` (SHA256 fallback)
- `src/memory_core/scoring.rs` — 26 externalized `ScoringParams`, type weights, word overlap, Jaccard
- `src/memory_core/storage/sqlite/` — SQLite backend:
  - `schema.rs` — table creation, additive migrations
  - `crud.rs` — store/retrieve/update/delete
  - `search.rs` — FTS5 BM25 + vector similarity
  - `advanced.rs` — 6-phase RRF pipeline (vector + FTS5 + rerank + refinement + graph + abstention)
  - `graph.rs` — relationship traversal (BFS, max_hops)
  - `lifecycle.rs` — TTL, sweep, feedback
  - `session.rs` — checkpoint, profile, session_info
  - `admin.rs` — health/export/import/stats, backup/restore
  - `helpers.rs` — retry logic, intent classification, cache keys, FTS5 query building

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
4. **Benchmark** (if touching scoring/search/storage): `cargo run --release --bin locomo_bench -- --samples 2 --scoring-mode word-overlap` — no regressions without justification

Run `prek run` for gates 1-3.

## Post-Implementation Checklist

- [ ] Quality gates pass (`prek run`)
- [ ] Benchmark shows no regression (if applicable)
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
- `conductor/` contains product planning docs — not runtime code
- Model files (~134 MB) auto-download on first use; `~/.mag/models/` preferred, `~/.romega-memory/models/` legacy
- `GRAPH_NEIGHBOR_FACTOR=0.1` — graph enrichment Phase 5 re-enabled at conservative factor; guarded by `if > 0.0`
- Git hooks do NOT fire under jj — run `prek run` explicitly before pushing

## Tool-Specific Notes

### Claude Code

- The `/jj` skill handles commit/push/PR workflows — use it instead of raw jj commands when available
- Use `/simplify` after completing implementation work to review for quality
- `isolation: "worktree"` on Agent tool creates git worktrees for parallel work; JJ workspaces preferred when available
- Session-start hooks can inject orchestrator mode — see chief-of-staff plugin

### Codex (OpenAI)

<!-- Add Codex-specific guidance here as needed -->
