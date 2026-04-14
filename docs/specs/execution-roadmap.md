# Execution Roadmap
<!-- Generated: 2026-04-14 | Baseline: v0.1.9-dev | Horizon: v0.2.2 -->

This document is the single source of truth for planned structural improvements to MAG. It covers four phases spanning v0.1.9 through v0.2.2, 15 pull requests, their dependencies, quality gates, risk mitigations, and parked items.

---

## Guiding Principles

- **Trait-first design** — add new traits and impls; never break existing signatures.
- **Benchmark-gated merges** — any PR touching scoring, search, or storage passes `./scripts/bench.sh --gate` before merge; a >5 pp regression blocks the PR.
- **One group at a time** — structural moves happen in atomic, test-passing steps, not big-bang rewrites.
- **Additive PRs only** — no removal of existing public APIs until a successor has been proven stable for one release.
- **Quality gates every PR** — `prek run` (fmt + clippy + tests) passes before push.

---

## Relationship to trait-surface.md

This roadmap (Phases 1-4) delivers **2 of the 7 substrate traits** defined in `trait-surface.md` — `ScoringStrategy` (PR-2a/2d) and `Reranker` (PR-2b) — plus structural decomposition of the four largest files and one reference backend (`MemoryStorage`). PR-4a-i adds a third trait (`RetrievalStrategy`), aligned with `trait-surface.md` §3.2's signature.

The full `substrate/` module — the remaining 5 traits (`FusionStrategy`, `Scorer`, `LifecyclePolicy`, `ConsolidationStrategy`, `IngestionPipeline`), the `SearchPipeline` and `WritePipeline` orchestrators, `MemoryStore` supertrait, blanket impls for backward compatibility, and the 5-phase deprecation path (Define, Implement, Wire, Deprecate, Remove) — is the **v0.3.x campaign**. `trait-surface.md` is the design reference for that campaign. It is not superseded by this roadmap.

Implementation agents working on v0.2.x should treat this roadmap as authoritative. When this roadmap and `trait-surface.md` conflict on trait signatures or module layout, this roadmap governs for Phases 1-4. For work beyond v0.2.2, `trait-surface.md` governs.

---

## Phase 1 — Clean House (target: v0.1.9)

Four independent PRs that can be opened and merged in parallel. No benchmark risk on 1a/1b/1d; 1c is benchmark-gated.

### PR-1a: Wire McpToolMode::Minimal list_tools filter

**Complexity**: S | **Risk**: Low | **Benchmark gate**: No

**Problem** (`src/mcp_server.rs`, lines 54–56): `McpToolMode::Minimal` is declared and stored but the rmcp proc-macro-generated router has no `list_tools` override, so the flag is a no-op at runtime. Clients that pass `--mcp-tools=minimal` see all 19 tools regardless.

**Scope**:
- Implement a `list_tools` override on the MCP handler struct that filters `TOOL_REGISTRY` to the 4 unified facades (`memory`, `memory_manage`, `memory_session`, `memory_admin`) when `mode == McpToolMode::Minimal`.
- Remove the "NOTE: Minimal mode filtering is not yet wired" comment from the `McpToolMode` doc block.
- Add a unit test asserting the filtered set contains exactly the 4 facade names.

**Acceptance**: `prek run` passes; `cargo test --all-features` includes the new test; comment is gone. Unit test asserting `McpToolMode::Full` (or default) returns all 19 registered tools from `TOOL_REGISTRY`.

---

### PR-1b: Fix AGENTS.md tool count

**Complexity**: S | **Risk**: None | **Benchmark gate**: No

**Problem** (`AGENTS.md`, Architecture section): The line `src/mcp_server.rs — MCP stdio server (16 tools)` is stale. `TOOL_REGISTRY` now has 19 entries (15 legacy + 4 unified facades).

**Scope**:
- Update the AGENTS.md reference from "16 tools" to "19 tools".
- Optionally clarify that `--mcp-tools=minimal` narrows the advertised set to 4.

**Acceptance**: AGENTS.md reflects the current registry count.

---

### PR-1c: Parallelize sub-queries in advanced_search (Issue #121)

**Complexity**: M | **Risk**: Medium | **Benchmark gate**: Yes

**Problem** (`src/memory_core/storage/sqlite/advanced.rs`, lines 1541–1559): The query-decomposition loop runs sub-queries sequentially with a `TODO(#121)` comment. The comment identifies two blockers: uncertain concurrent-reader support in the pool, and serial `seen_ids` accumulation across iterations.

**Scope**:
- Verify `ConnPool` supports concurrent reader connections (inspect `conn_pool.rs`). If not, add a reader-pool expansion path first.
- Replace the sequential `for sub_query` loop with `futures::future::join_all` (or `tokio::task::JoinSet`) issuing sub-queries concurrently.
- Merge results after join: collect all sub-results, then apply the `seen_ids` dedup and score-max logic in a single pass.
- Remove the `// TODO(#121)` comment.

**Risk mitigations**:
- Run the parallel path only for Conceptual/General intents (already the path that reaches decomposition).
- Confirm with `cargo test --all-features` that no test uses a single-connection in-memory pool where concurrent access would deadlock.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --gate` shows no regression; `TODO(#121)` comment removed. Unit test verifying that when two parallel sub-queries return the same `memory_id` with different scores, the result contains exactly one entry with `score = max(scores)`.

---

### PR-1d: Split admin.rs into admin/ subdirectory

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No

**Problem**: `src/memory_core/storage/sqlite/admin.rs` (1,619 lines) contains four independent trait impl groups (`BackupManager`, `MaintenanceManager`, `WelcomeProvider`, `StatsProvider`) with no cross-group coupling. The module-decomposition spec (Section 2d, Section 3d) identifies this as the easiest structural split in the codebase.

**Scope — target layout** (per `module-decomposition.md` §2d):

| File | Content |
|---|---|
| `sqlite/admin/mod.rs` | Re-exports, constants (`MAX_BACKUPS`, `BACKUP_INTERVAL_SECS`, `BACKUP_PREFIX`, `BACKUP_SUFFIX`) |
| `sqlite/admin/backup.rs` | `backups_dir()`, `collect_backup_entries()`, `create_backup_sync()`, `rotate_backups_sync()`, `list_backups_sync()`, `restore_backup_sync()`, `needs_backup()`, `impl BackupManager for SqliteStorage` |
| `sqlite/admin/maintenance.rs` | `impl MaintenanceManager for SqliteStorage`: `check_health()`, `consolidate()`, `compact()`, `auto_compact()`, `clear_session()`, `estimate_tokens()` |
| `sqlite/admin/welcome.rs` | `impl WelcomeProvider for SqliteStorage`: `welcome()`, `welcome_scoped()` |
| `sqlite/admin/stats.rs` | `impl StatsProvider for SqliteStorage`: `type_stats()`, `session_stats()`, `weekly_digest()`, `access_rate_stats()` |

**Execution order** (move one group at a time, verify `prek run` between each):
1. Create `admin/` directory and `admin/mod.rs` with re-exports
2. Move `impl BackupManager` block → `admin/backup.rs`
3. Move `impl MaintenanceManager` block → `admin/maintenance.rs`
4. Move `impl WelcomeProvider` block → `admin/welcome.rs`
5. Move `impl StatsProvider` block → `admin/stats.rs`
6. Replace `admin.rs` with `admin/mod.rs`; update parent `mod.rs` declaration

**File size exceptions**: `admin/maintenance.rs` (~580 lines) and `admin/welcome.rs` (~490 lines) exceed the 500-line target. Both exceptions are justified in `module-decomposition.md` §4: `MaintenanceManager` methods share `estimate_tokens()` helper with strong semantic cohesion; `welcome()` and `welcome_scoped()` are tightly coupled with shared JSON output shape.

**Risk mitigations**:
- Each step is a pure move — no logic changes. The four trait impl groups are completely independent.
- `prek run` after each step catches compilation issues immediately.
- Admin logic is not on the search path; no benchmark gate needed.

**Acceptance**: `prek run` passes; `admin.rs` replaced by `admin/` directory with 5 files; no public API surface changes.

---

## Phase 2 — Scoring Decoupling + Structural Cleanup (target: v0.2.0)

Four PRs with a serial dependency chain. PR-2a and PR-2b can be opened in parallel; PR-2c starts only after both land; PR-2d starts after PR-2c.

```
PR-2a ──┐
        ├──> PR-2c ──> PR-2d
PR-2b ──┘
```

### PR-2a: Extract ScoringStrategy trait

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No (additive)

**Problem**: Scoring logic is embedded in `SqliteStorage` and tied to the concrete `ScoringParams` struct. Phase 3 and Phase 4 need a scoring abstraction that can be swapped without rebuilding storage.

**Scope**:
- Define a `ScoringStrategy` trait in `src/memory_core/scoring.rs` (or a new `scoring_strategy.rs`).
- Trait surface (initial): `fn score(&self, candidate: &RankedSemanticCandidate, query: &str, params: &ScoringParams) -> f64`.
- Implement `DefaultScoringStrategy` that wraps the existing score-refinement logic from `advanced.rs` Phase 4 (`fuse_refine_and_output`).
- No behavioral changes; the existing path delegates to `DefaultScoringStrategy`.
- Design for extensibility: the trait should accept a context bag (query, options, scoring params) so Phase 4's `KeywordOnlyStrategy` can ignore unused fields without a breaking signature change.

**Acceptance**: `prek run` passes; no existing test changes behavior; trait is documented with a doc comment explaining the extension point.

---

### PR-2b: Extract Reranker trait boundary (Issue #119)

**Complexity**: M | **Risk**: Medium | **Benchmark gate**: Yes

**Problem**: `CrossEncoderReranker` (`src/memory_core/reranker.rs`) is a concrete struct referenced directly in `SqliteStorage` via `#[cfg(feature = "real-embeddings")]`. There is no trait boundary, making alternative reranker implementations (e.g., a no-op pass-through for testing, or a future LLM-based reranker) impossible without modifying `SqliteStorage`.

**Scope**:
- Define a `Reranker` trait in `src/memory_core/reranker.rs`:
  - `async fn rerank(&self, query: &str, candidates: &[RankedSemanticCandidate]) -> Result<HashMap<String, f32>>`.
- Implement `Reranker` for `CrossEncoderReranker` (feature-gated as today).
- Add a `NoOpReranker` that returns an empty scores map (used in tests and non-`real-embeddings` builds).
- Change `SqliteStorage::reranker` field from `Option<Arc<CrossEncoderReranker>>` to `Option<Arc<dyn Reranker + Send + Sync>>`.

**Risk mitigations**:
- The cross-encoder path is feature-gated; the trait change is contained to a `cfg` block.
- Run full benchmark suite after landing to verify the reranker-enabled path scores identically.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --gate` shows no regression; `CrossEncoderReranker` is not directly referenced in `mcp_server.rs` or `SqliteStorage` field types.

---

### PR-2c: sqlite/mod.rs structural extraction

**Complexity**: L | **Risk**: Medium | **Benchmark gate**: Yes (structural sanity check)

**Problem**: `src/memory_core/storage/sqlite/mod.rs` (1,281 lines) holds the `SqliteStorage` struct definition, all constructor/builder methods, cache management logic (`CachedQuery`, `QueryCache`, `invalidate_cache_selective`, `invalidate_query_cache`), hot-cache lifecycle methods (`refresh_hot_cache`, `start_hot_cache_refresh_task`, `ensure_hot_cache_ready`), the `add_relationship` method, and the `stats`/`optimize` utility methods. These concerns are distinct and the file is long enough to impede navigation.

The trait implementations themselves are already distributed across submodules (`crud.rs`, `search.rs`, `advanced.rs`, `graph.rs`, `lifecycle.rs`, `session.rs`, `admin.rs`, etc.) — no trait redistribution is needed.

**Target file layout after this PR**:

| New file | Content |
|---|---|
| `storage.rs` | `SqliteStorage` struct definition, `InitMode`, `ConnPool` wiring, all constructor/builder methods (`new`, `new_default`, `new_with_path`, `with_reranker`, `with_scoring_params`, `set_scoring_params`, `scoring_params`, `optimize`) |
| `cache.rs` | `CachedQuery`, `QueryCache`, `new_query_cache`, `invalidate_query_cache`, `invalidate_cache_selective`, `cache_entry_could_be_affected` |
| `hot_cache_mgmt.rs` | `refresh_hot_cache`, `refresh_hot_cache_best_effort`, `ensure_hot_cache_ready`, `start_hot_cache_refresh_task` (orchestration logic; `HotTierCache` struct stays in `hot_cache.rs`) |
| `relationships.rs` | `add_relationship`, `graph_edge_stats` |
| `io.rs` | `stats`, any remaining bulk-read helpers currently in `mod.rs` |

`mod.rs` becomes a thin re-export facade after extraction.

**Execution order** (move one group at a time, verify `prek run` between each):
1. Extract cache types/functions → `cache.rs`
2. Extract hot-cache management → `hot_cache_mgmt.rs`
3. Extract relationship methods → `relationships.rs`
4. Extract I/O helpers → `io.rs`
5. Extract struct + constructors → `storage.rs`
6. Reduce `mod.rs` to re-exports

**Risk mitigations**:
- Each step is a pure move — no logic changes. Compile after every step.
- Use `pub(super)` visibility where submodules already rely on it; adjust visibility declarations to match the new module boundary.
- The existing 500+ tests in `tests.rs` cover this surface; no test changes expected.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --gate` shows no regression (per `module-decomposition.md` §6 test gates — even for structural moves, the gate catches subtle visibility/import errors that could alter behavior); `mod.rs` is under 100 lines of re-exports; no public API surface changes.

---

### PR-2d: Inject ScoringStrategy into SqliteStorage

**Complexity**: M | **Risk**: Medium | **Benchmark gate**: Yes (10-sample gate required — manual; CI runs 2-sample fast mode)

**Depends on**: PR-2a (trait definition) and PR-2c (storage struct extracted, easier to modify)

**Problem**: After PR-2a defines the `ScoringStrategy` trait, `SqliteStorage` should hold a `Box<dyn ScoringStrategy>` (or `Arc`) field and delegate Phase 4 scoring to it. This severs the hard coupling between the storage struct and the scoring implementation, enabling Phase 3's `MemoryStorage` to use a different strategy without duplicating code.

**Scope**:
- Add `scoring_strategy: Arc<dyn ScoringStrategy + Send + Sync>` field to `SqliteStorage`.
- Default-initialize to `Arc::new(DefaultScoringStrategy::new())`.
- Add `with_scoring_strategy` builder method.
- Thread the strategy into `fuse_refine_and_output` (currently in `advanced.rs`) by passing it through the call chain or storing it in `SqliteStorage` for sub-function access.
- Update grid-search and benchmark harnesses that call `set_scoring_params` to still work (they configure the `ScoringParams` struct, which `DefaultScoringStrategy` reads — no change needed there).

**Risk mitigations**:
- Keep `ScoringParams` as a plain data struct (not part of the trait) so grid-search continues to work unmodified.
- Benchmark gates catch any scoring drift from the delegation layer.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --samples 10 --notes "PR-2d pre-merge"` shows no regression (10-sample gate required — PR-2d threads scoring delegation through the hot path, which is an algorithmic-path change, not a code move; 2-sample noise floor is insufficient to detect subtle parameter-passing bugs); `ScoringStrategy` is injected and documented.

---

## Phase 3 — Prove the Substrate (target: v0.2.1)

Three PRs with internal dependencies. PR-3a is independent. PR-3b depends on PR-2d (needs `ScoringStrategy` injection to wire `DefaultScoringStrategy`). PR-3c depends on PR-3b (conformance suite requires `MemoryStorage` to exist). PR-3a can run in parallel with PR-3b; PR-3c is serial after PR-3b.

```
PR-3a (independent, parallel with PR-3b)
PR-3b (depends on PR-2d) ──> PR-3c
```

### PR-3a: Strategy comparison benchmark harness

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No (adds harness, no behavior change)

**Problem**: There is currently no tooling to compare scoring strategies head-to-head against the LoCoMo benchmark. Adding strategies without comparative tooling means regressions can only be caught after the fact.

**Governing spec**: `benchmark-harness.md` defines the detailed design for the strategy comparison feature. PR-3a implements that spec. Where this section summarizes, the benchmark-harness spec is authoritative.

**Scope** (per `benchmark-harness.md` §1-§5):
- Add `--strategy <name>` and `--list-strategies` flags to the existing `locomo_bench` binary (not a standalone binary).
- Create `benches/locomo/strategies.rs` with the `STRATEGIES` registry, `find_strategy()`, `list_strategies()`, and `build_storage()` functions.
- Create `benches/bench_utils/stats.rs` with `percentile_ms()` P95 latency helper.
- Add `strategy: String` and `p95_query_ms: f64` fields to `LoCoMoSummary` in `benches/locomo/types.rs` (both `#[serde(default)]`).
- Create `docs/benchmarks/baselines.json` bootstrapped from `methodology.md` values (schema per `benchmark-harness.md` §4).
- Update `bench.sh --gate` to read the baseline from `baselines.json` instead of CSV grep (per `benchmark-harness.md` §4).
- Create `scripts/compare_strategies.py` (Python 3 stdlib only) for strategy comparison reports.
- Extend `bench.sh` with `--compare A B` mode that runs two strategy invocations and calls `compare_strategies.py`.

**Acceptance**: `cargo run --release --bin locomo_bench -- --strategy sqlite-v1 --list-strategies` works; `--strategy sqlite-v1` (default) produces scores identical to the unpatched binary; `baselines.json` exists and `bench.sh --gate` reads from it; `compare_strategies.py` produces well-formed JSON and Markdown reports; CSV rows are appended with `strategy=<id>` in the notes field.

---

### PR-3b: In-memory HashMap backend (MemoryStorage)

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No

**Problem**: All tests run against SQLite (`:memory:`). An in-memory `HashMap`-backed storage backend would enable faster unit tests for pure logic, make conformance testing explicit, and serve as a reference implementation of the storage traits.

**Scope**:
- Add `src/memory_core/storage/memory/mod.rs` implementing `MemoryStorage`.
- Implement the core trait set: `Storage`, `Retriever`, `Searcher` (text-match only, no FTS5/vector), `Deleter`, `Updater`, `Lister`, `Tagger`, `StatsProvider`.
- Traits that are SQLite-specific (`AdvancedSearcher`, `PhraseSearcher`) can be stubbed with `unimplemented!()` for now.
- Use `Arc<RwLock<HashMap<String, StoredMemory>>>` internally.
- Inject `DefaultScoringStrategy` (from PR-2a/2d) for any scoring that happens at retrieval time.

**Acceptance**: `prek run` passes; at least 10 unit tests cover `MemoryStorage` CRUD paths.

---

### PR-3c: Shared backend conformance suite

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No

**Depends on**: PR-3b (`MemoryStorage` must exist to instantiate the suite against it)

**Problem**: Without a shared test suite, the `MemoryStorage` and `SqliteStorage` backends can diverge silently. A conformance suite ensures both backends exhibit identical behavior for the traits they share.

**Scope**:
- Add `tests/storage_conformance.rs`.
- Define a macro or generic function `run_conformance_suite<S: Storage + Retriever + ...>(storage: S)` that runs a standard set of assertions: store/retrieve round-trip, update, delete, tag search, list ordering, stats counts.
- Instantiate it for both `SqliteStorage::new_with_path(":memory:")` and `MemoryStorage::new()`.
- Failures in conformance tests are treated as blocking bugs, not warnings.

**Trait scope note**: The conformance suite tests the trait subset that `MemoryStorage` implements: `Storage`, `Retriever`, `Deleter`, `Updater`, `Lister`, `Searcher`, `Tagger`, `StatsProvider`. It does NOT include `AdvancedSearcher` or `PhraseSearcher` in the generic bound — `MemoryStorage` stubs these with `unimplemented!()` (per PR-3b), so including them would panic at test time. The suite covers the common-subset contract; SQLite-specific traits are tested by the existing `tests.rs` suite.

**Acceptance**: `cargo test --all-features` runs the conformance suite against both backends; all tests pass.

---

## Phase 4 — Exercise & Decompose (target: v0.2.2)

Four PRs. PR-4a-i depends on PR-2a/PR-2d and can be opened after Phase 2 merges. PR-4b can be opened any time after Phase 1 — it has no dependency on Phases 2 or 3. PR-4a-ii depends on PR-4a-i. PR-4c depends on PR-4b.

```
PR-4a-i ──> PR-4a-ii
PR-4b   ──> PR-4c
```

### PR-4a-i: Define RetrievalStrategy trait + FullPipelineStrategy reference impl

**Complexity**: M | **Risk**: Low | **Benchmark gate**: No (additive — no behavior change)

**Depends on**: PR-2a, PR-2d (ScoringStrategy injection in place)

**Problem**: The current pipeline always runs the full 6-phase RRF pipeline (vector + FTS5 + rerank + refinement + graph + abstention). Before alternative retrieval paths can be added (PR-4a-ii), the retrieval abstraction must be defined.

**Scope**:
- Define a `RetrievalStrategy` trait aligned with `trait-surface.md` §3.2 (returns raw `CandidateSet`, not fully-scored `Vec<SemanticResult>`):
  - `fn name(&self) -> &str`
  - `async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet>`
- **Design alignment note**: PR-4a-i's trait design MUST match `trait-surface.md` §3.2. The trait returns unscored `CandidateSet` to preserve the fusion step. Strategies produce raw candidates; fusion, scoring, and abstention happen downstream in the pipeline composition model. This ensures `RetrievalStrategy` is composable with multi-strategy pipelines in the v0.3.x substrate campaign.
- Implement `FullPipelineStrategy` as a reference impl that wraps the existing 6-phase path, collecting candidates into a `CandidateSet`.
- No behavioral changes — the existing path delegates to `FullPipelineStrategy` and the downstream fusion/scoring/abstention steps remain unchanged.

**Acceptance**: `prek run` passes; no existing test changes behavior; `RetrievalStrategy` trait and `FullPipelineStrategy` impl are documented with doc comments.

---

### PR-4a-ii: Implement KeywordOnlyStrategy + wire dispatch

**Complexity**: M | **Risk**: High | **Benchmark gate**: Yes (10-sample gate required — manual; CI runs 2-sample fast mode)

**Depends on**: PR-4a-i (trait definition)

**Problem**: For Keyword-intent queries, the vector and rerank phases add latency with marginal quality benefit. A `KeywordOnlyStrategy` allows the query classifier to select a simpler, faster path.

**Scope**:
- Implement `KeywordOnlyStrategy`: FTS5 BM25 only, no embedding, no reranker. Returns raw FTS candidates as a `CandidateSet` (per the trait contract). Scoring via `DefaultScoringStrategy` happens downstream in the pipeline, not inside the strategy.
- Register both strategies in `SqliteStorage`; dispatch to `KeywordOnlyStrategy` when `intent.primary == QueryIntent::Keyword`.
- The 10-sample benchmark gate guards against latency/quality tradeoffs that hurt overall LoCoMo score.

**Risk mitigations**:
- The `name()`-based dispatch means the full pipeline is preserved for all other intents.
- Run `./scripts/bench.sh --samples 10 --notes "PR-4a-ii pre-merge"` to establish a baseline; gate at >5 pp regression.
- If keyword queries show a quality drop, tune `KeywordOnlyStrategy` scoring weights before merging.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --samples 10` passes with no >5pp regression; keyword-intent queries use `KeywordOnlyStrategy` (verifiable via tracing log).

---

### PR-4b: Split mcp_server.rs into mcp/ module

**Complexity**: L | **Risk**: Medium | **Benchmark gate**: No

**Problem**: `src/mcp_server.rs` (2,709 lines) contains the tool registry, MCP instructions, validation helpers, `McpToolMode`, and every tool handler. It is the largest single file in the codebase.

**Scope — target module layout**:

| File | Content |
|---|---|
| `src/mcp/mod.rs` | Re-exports, `McpToolMode`, `MCP_INSTRUCTIONS`, `ToolMeta`, `TOOL_REGISTRY` |
| `src/mcp/validation.rs` | `require_finite`, `MAX_RESULT_LIMIT`, `MAX_BATCH_SIZE` |
| `src/mcp/tools/storage.rs` | `memory_store`, `memory_store_batch`, `memory_retrieve`, `memory_delete`, `memory_update`, `memory` facade |
| `src/mcp/tools/search.rs` | `memory_search`, `memory_list` |
| `src/mcp/tools/relations.rs` | `memory_relations` |
| `src/mcp/tools/lifecycle.rs` | `memory_feedback`, `memory_lifecycle` |
| `src/mcp/tools/session.rs` | `memory_checkpoint`, `memory_remind`, `memory_lessons`, `memory_profile`, `memory_session_info`, `memory_session` facade |
| `src/mcp/tools/admin.rs` | `memory_admin` facade |
| `src/mcp/tools/manage.rs` | `memory_manage` facade |

**Risk mitigations**:
- The thin-wrapper pattern is documented in `module-decomposition.md` §2a with a working code example. The `#[tool_router] impl McpMemoryServer` block stays in `mcp/mod.rs` with 1-3 line wrapper methods that delegate to free functions in `tools/*.rs`. Verify the rmcp 0.16 `#[tool_router]` proc-macro accepts cross-module handler delegation in the first commit before proceeding with the full extraction.
- If the proc macros require co-location with the impl block, consolidate tool method bodies into submodules called from a single `impl MagServer` in `mcp/mod.rs`.
- Run `cargo test --all-features` including MCP smoke tests (`tests/mcp_smoke.rs`) after each file split.

**Acceptance**: `prek run` passes; `src/mcp_server.rs` is either removed or reduced to a thin re-export shim; MCP smoke tests pass. Update AGENTS.md Architecture section to reflect `src/mcp/` module structure (replacing `src/mcp_server.rs` references).

---

### PR-4c: Split advanced.rs into pipeline/ subdirectory

**Complexity**: M | **Risk**: Medium | **Benchmark gate**: Yes (structural sanity check)

**Depends on**: PR-4b (establishes the module-split pattern; lessons applied here)

**Problem**: `advanced.rs` (1,739 lines, 6-phase pipeline + decomposition) is the last major god file after PR-1d (which already split `admin.rs` in Phase 1). The target layout follows `module-decomposition.md` §2c and §3c, which provide line-by-line function assignments to the `pipeline/` subdirectory.

**Pre-split requirement**: Audit shared helper types across `advanced.rs` and the target `pipeline/` files before splitting. If types like `RankedSemanticCandidate` or utility functions are shared across multiple target files, extract them to `mod.rs` re-exports or a `types.rs` first. This avoids circular imports during the split.

**Scope — advanced.rs → pipeline/** (per `module-decomposition.md` §2c):

| File | Content |
|---|---|
| `sqlite/pipeline/mod.rs` | Module re-exports, `ADVANCED_FTS_CANDIDATE_*` constants, `advanced_fts_candidate_limit()` |
| `sqlite/pipeline/retrieval.rs` | `collect_vector_candidates()` (Phase 1), `collect_fts_candidates()` (Phase 2) |
| `sqlite/pipeline/rerank.rs` | `compute_cross_encoder_scores()` [cfg(real-embeddings)] |
| `sqlite/pipeline/fusion.rs` | `fuse_refine_and_output()` Phase 3 RRF block: RRF scoring, dual-match boost, cross-encoder blend |
| `sqlite/pipeline/scoring.rs` | `refine_scores()` (Phase 4) |
| `sqlite/pipeline/enrichment.rs` | `enrich_graph_neighbors()` (Phase 5), `expand_entity_tags()` (Phase 5b) |
| `sqlite/pipeline/abstention.rs` | Abstention + dedup block (Phase 6), `merge_hot_cache_results()`, `merge_semantic_result()`, `merge_semantic_metadata()` |
| `sqlite/pipeline/decomp.rs` | `run_single_query_pipeline()`, query decomposition orchestration |

`advanced.rs` residual (~350 lines): `impl AdvancedSearcher for SqliteStorage` (the `advanced_search` body, cache read/write) + tests module. The residual calls into `pipeline/` functions.

**Risk mitigations**:
- This touches the search scoring path. Run `./scripts/bench.sh --gate` after the split to catch any subtle visibility/import errors.
- Each step is a pure move — no logic changes. Compile after every step.
- The module-decomposition spec (§3c) provides exact line-number assignments for every function.

**Acceptance**: `prek run` passes; `./scripts/bench.sh --gate` shows no regression; file sizes are all under 500 lines (with `admin/maintenance.rs` ~580 and `admin/welcome.rs` ~490 exceptions per `module-decomposition.md` §4); no behavioral changes. Update AGENTS.md Architecture section to reflect `sqlite/pipeline/` and `sqlite/admin/` module structures.

---

## Dependency Graph

```
Phase 1 (parallel)
├── PR-1a  Wire McpToolMode::Minimal
├── PR-1b  Fix AGENTS.md tool count
├── PR-1c  Parallelize sub-queries [bench gate]
└── PR-1d  Split admin.rs → admin/

Phase 2 (serial chain)
├── PR-2a  ScoringStrategy trait ──┐
├── PR-2b  Reranker trait [bench]  ├──> PR-2c  sqlite/mod.rs extraction [bench] ──> PR-2d  Inject strategy [10-sample bench]
└─────────────────────────────────┘

Phase 3 (after Phase 2 completes)
├── PR-3a  Strategy comparison harness (independent, parallel with PR-3b)
├── PR-3b  MemoryStorage backend (depends on PR-2d) ──> PR-3c  Conformance suite
└──────────────────────────────────────────────────────────────────────────────

Phase 4
├── PR-4a-i   Define RetrievalStrategy trait + FullPipelineStrategy (after Phase 2)
├── PR-4a-ii  KeywordOnlyStrategy + dispatch [10-sample bench] (depends on PR-4a-i)
├── PR-4b     Split mcp_server.rs (no dependency — can start after Phase 1) ──> PR-4c  Split advanced.rs → pipeline/ [bench gate]
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
```

---

## Risk Registry

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| 1 | PR-2c breaks the test suite during file moves | Medium | High | Move one group at a time; run `prek run` after each step; no logic changes, only moves |
| 2 | PR-4a-ii causes LoCoMo regression on keyword or adversarial categories | Medium | High | 10-sample gate required; tune `KeywordOnlyStrategy` weights before merge; rollback path is removing the dispatch |
| 3 | rmcp proc-macro incompatibility blocks PR-4b module split | Medium | Medium | Thin-wrapper pattern documented in `module-decomposition.md` §2a with code example; verify `#[tool_router]` accepts cross-module delegation in first commit; fallback is consolidating method bodies in submodules called from a single `impl` block |
| 4 | `ScoringStrategy` trait surface too narrow for Phase 4's `KeywordOnlyStrategy` | Medium | Medium | Design trait with a context bag parameter from the start (PR-2a); extend rather than break if needed |
| 5 | `MemoryStorage` conformance suite reveals hidden invariants in SQLite backend | Low | Low (positive) | Treat as a feature: invariants should be documented and enforced; conformance failures are bugs to fix, not tests to skip |
| 6 | Visibility cascade when moving `SqliteStorage` to `storage.rs` (PR-2c) — every `super::SqliteStorage` reference in sibling submodules (`advanced.rs`, `crud.rs`, `search.rs`, `graph.rs`, `session.rs`, `lifecycle.rs`) breaks | High | Medium | Re-export `SqliteStorage` from `mod.rs` (`pub use storage::SqliteStorage;`) so existing `super::SqliteStorage` paths resolve. Apply this re-export as the first step of PR-2c, before moving any methods. |
| 7 | PR-2b benchmark warning blocks entire PR-2c/PR-2d chain — a >2pp delta triggers a mandatory 10-sample run, stalling the serial chain | Medium | High | If PR-2b warns, investigate the delta before proceeding. A reranker-path regression is a real signal, not noise. Do not block the chain on noise — but do not ignore a genuine regression. If the 10-sample run confirms no regression, proceed; if it confirms regression, fix PR-2b before continuing. |
| 8 | PR-1c scope expansion if `ConnPool` lacks concurrent reader support — the "if not, add a reader-pool expansion path first" clause is unscoped work hiding inside a Phase 1 quick-win | Medium | Medium | Spike `ConnPool` reader availability (`conn_pool.rs` inspection) before committing to PR-1c scope. If concurrent readers are not supported, either expand PR-1c to L-complexity with pool changes, or defer PR-1c to Phase 2 and replace it with a smaller Phase 1 win. |

---

## Milestone Checkpoints

### v0.1.9 checkpoint
- [ ] PR-1a merged: `--mcp-tools=minimal` filters the tool list at runtime
- [ ] PR-1b merged: AGENTS.md tool count is accurate
- [ ] PR-1c merged: sub-query parallelism active; `TODO(#121)` gone; no benchmark regression
- [ ] PR-1d merged: `admin.rs` split into `admin/` subdirectory (4 files + mod.rs)

### v0.2.0 checkpoint
- [ ] PR-2a merged: `ScoringStrategy` trait and `DefaultScoringStrategy` in place
- [ ] PR-2b merged: `Reranker` trait in place; `CrossEncoderReranker` implements it; no benchmark regression
- [ ] PR-2c merged: `sqlite/mod.rs` reduced to re-export facade; `storage.rs`, `cache.rs`, `hot_cache_mgmt.rs`, `relationships.rs`, `io.rs` all exist and pass tests; benchmark gate passes
- [ ] PR-2d merged: `ScoringStrategy` injected into `SqliteStorage`; 10-sample benchmark gate passes

### v0.2.1 checkpoint

Prerequisite: v0.2.0 checkpoint must be fully complete (all Phase 2 PRs merged) before PR-3b implementation begins. PR-3b depends on PR-2d's `ScoringStrategy` injection.

- [ ] PR-3a merged: strategy comparison harness with `--strategy` flag on `locomo_bench`
- [ ] PR-3b merged: `MemoryStorage` backend with 10+ unit tests
- [ ] PR-3c merged: conformance suite passes for both backends

### v0.2.2 checkpoint
- [ ] PR-4a-i merged: `RetrievalStrategy` trait defined (aligned with `trait-surface.md` §3.2); `FullPipelineStrategy` reference impl in place
- [ ] PR-4a-ii merged: `KeywordOnlyStrategy` dispatched for keyword intent; 10-sample gate passed
- [ ] PR-4b merged: `mcp_server.rs` split into `mcp/` module; MCP smoke tests pass
- [ ] PR-4c merged: `advanced.rs` split into `pipeline/` subdirectory; benchmark gate passes

---

## Quality Gate Summary

| Gate | When | Command |
|---|---|---|
| Format | Every PR | `cargo fmt --all -- --check` |
| Lint | Every PR | `cargo clippy --all-targets --all-features -- -D warnings` |
| Tests | Every PR | `cargo test --all-features` |
| Combined | Every PR | `prek run` (runs all three above) |
| Benchmark 2-sample | PRs touching scoring/search/storage | `./scripts/bench.sh --gate` (warns >2 pp, fails >5 pp) |
| Benchmark 10-sample | If 2-sample gate warned; required for PR-2d, PR-4a-ii | `./scripts/bench.sh --samples 10 --notes "pre-merge <pr>"` |

10-sample gates are manual pre-merge checks. CI enforces 2-sample fast gates automatically. The 10-sample requirement for PR-2d and PR-4a-ii must be verified by the developer before merge and logged to the benchmark CSV.

Results must be appended to `docs/benchmarks/benchmark_log.csv` using `bench.sh` (not raw `cargo run`).

The benchmark gate compares against `docs/benchmarks/baselines.json` (see `benchmark-harness.md` §4). Before PR-3a lands, the gate uses the existing CSV-grep baseline.

---

## Branch Naming and Rebase Protocol

This repo is jj-colocated. Use jj bookmarks for all PR branches. For the serial dependency chains (PR-2a through PR-2d, PR-4a-i through PR-4a-ii), the following protocol applies:

### Bookmark names

| PR | Bookmark |
|---|---|
| PR-1a | `fix/mcp-minimal-filter` |
| PR-1b | `fix/agents-tool-count` |
| PR-1c | `feat/parallel-subqueries` |
| PR-1d | `refactor/admin-split` |
| PR-2a | `refactor/scoring-strategy` |
| PR-2b | `refactor/reranker-trait` |
| PR-2c | `refactor/sqlite-extraction` |
| PR-2d | `refactor/scoring-injection` |
| PR-3a | `refactor/strategy-harness` |
| PR-3b | `refactor/memory-storage` |
| PR-3c | `refactor/conformance-suite` |
| PR-4a-i | `refactor/retrieval-strategy` |
| PR-4a-ii | `refactor/keyword-strategy` |
| PR-4b | `refactor/mcp-split` |
| PR-4c | `refactor/pipeline-split` |

### Rebase protocol for serial chains

When an earlier PR in a dependency chain merges to main:

```bash
# After PR-2a merges:
jj rebase -b refactor/sqlite-extraction -d main

# After PR-2c merges:
jj rebase -b refactor/scoring-injection -d main

# After PR-4a-i merges:
jj rebase -b refactor/keyword-strategy -d main
```

For parallel PRs (Phase 1, PR-3a/3b), no rebase coordination is needed — each is branched from main independently.

When subagents are dispatched to implement PRs in parallel on overlapping files, use `isolation: "worktree"` to prevent conflicting edits. The serial chain PRs (2a/2b → 2c → 2d) must NOT be implemented concurrently on the same worktree.

---

## Parked Items

These are noted but explicitly out of scope for this roadmap. File GitHub issues for each if they need tracking.

| Item | Reason parked |
|---|---|
| Voyage-4-nano ONNX as default embedder | Requires embedding dimension migration; separate campaign |
| Knowledge graph as primary retrieval path | Phase 5 graph enrichment at 0.1 factor is deliberately conservative; promote only after benchmarked improvement |
| LLM-based reranker | Requires `Reranker` trait (PR-2b) first; adds external dependency; cost model unclear |
| Admin HTTP API / daemon-mode REST expansion | Separate feature campaign; not a structural refactor |
| `conductor/` Gemini CLI cleanup | Legacy artifact; not actively maintained; low priority |
| Multi-file import streaming | Useful but orthogonal; file separately |

---

## Version Targets Summary

| Phase | Version | PRs | Focus |
|---|---|---|---|
| 1 | v0.1.9 | 1a, 1b, 1c, 1d | Quick wins: unblock Minimal mode, correct docs, parallelize sub-queries, split admin.rs |
| 2 | v0.2.0 | 2a, 2b, 2c, 2d | Scoring decoupling + `sqlite/mod.rs` structural cleanup |
| 3 | v0.2.1 | 3a, 3b, 3c | Prove the abstraction substrate: harness, in-memory backend, conformance |
| 4 | v0.2.2 | 4a-i, 4a-ii, 4b, 4c | Exercise strategies, decompose mcp_server.rs and advanced.rs |

---

## PR Summary Table

| PR | Phase | Complexity | Risk | Benchmark Gate | Depends On |
|---|---|---|---|---|---|
| PR-1a | 1 | S | Low | No | — |
| PR-1b | 1 | S | None | No | — |
| PR-1c | 1 | M | Medium | Yes (2-sample) | — |
| PR-1d | 1 | M | Low | No | — |
| PR-2a | 2 | M | Low | No (additive) | — |
| PR-2b | 2 | M | Medium | Yes (2-sample) | — |
| PR-2c | 2 | L | Medium | Yes (structural) | PR-2a, PR-2b |
| PR-2d | 2 | M | Medium | Yes (10-sample, manual) | PR-2a, PR-2c |
| PR-3a | 3 | M | Low | No (adds harness) | — |
| PR-3b | 3 | M | Low | No | PR-2d |
| PR-3c | 3 | M | Low | No | PR-3b |
| PR-4a-i | 4 | M | Low | No (additive) | PR-2a, PR-2d |
| PR-4a-ii | 4 | M | High | Yes (10-sample, manual) | PR-4a-i |
| PR-4b | 4 | L | Medium | No | — |
| PR-4c | 4 | M | Medium | Yes (structural) | PR-4b |
