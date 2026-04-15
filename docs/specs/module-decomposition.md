# Module Decomposition Spec
<!-- Status: VALIDATED | Verified against source: 2026-04-14 | Target: v0.2.x | Phase 2+3 confirmed design; Phase 4 (mcp/ and pipeline/ splits) in progress -->

## 0. Recon Corrections

The initial recon contained one critical error that affects the decomposition plan:

**WRONG:** `sqlite/mod.rs` holds 19 inline trait impls.
**CORRECT:** All trait impls are already distributed across submodules (`crud.rs`, `search.rs`,
`graph.rs`, `session.rs`, `lifecycle.rs`, `advanced.rs`, `admin.rs`). The `mod.rs` file's 1,281
lines consist of: `SqliteStorage` struct definition and constructors, cache management structs
and methods, hot-cache background task, relationship operations (`add_relationship`,
`try_auto_relate`, `try_create_temporal_edges`, `try_create_entity_edges`), bulk I/O methods
(`stats`, `export_all`, `import_all`), the `RankedSemanticCandidate` struct, and submodule
declarations + re-exports. There are no inline trait impls to extract — only concerns to separate.

---

## 1. Current State Inventory

| File | Lines | Problem |
|------|-------|---------|
| `src/mcp_server.rs` | 2,709 | Mixed: protocol, request types, validation, 19 tool bodies, server infra |
| `src/memory_core/storage/sqlite/advanced.rs` | 1,739 | Mixed: 7 pipeline phase functions, hot-cache helpers, query decomp orchestration, `impl AdvancedSearcher` |
| `src/memory_core/storage/sqlite/admin.rs` | 1,619 | Mixed: 4 independent trait impl groups (backup, maintenance, welcome/stats, stats sub-methods) |
| `src/memory_core/storage/sqlite/mod.rs` | 1,281 | Mixed: struct+constructors, cache management, hot-cache task, relationships, bulk I/O, shared types |

---

## 2. Target Module Tree

### 2a. MCP Server: `src/mcp/`

```
src/mcp/
├── mod.rs              ~400 lines   McpMemoryServer struct, #[tool_router] with 1-3 line wrappers,
│                                    McpToolMode enum, serve_stdio(), ServerHandler impl
├── protocol.rs         ~200 lines   MCP_INSTRUCTIONS const, TOOL_REGISTRY, CATEGORY_ORDER,
│                                    generate_protocol_markdown(), tool_registry_json(), ToolMeta struct
├── request_types.rs    ~350 lines   All *Request structs (StoreRequest, SearchRequest, ListRequest,
│                                    DeleteRequest, UpdateRequest, RelationsRequest, FeedbackRequest,
│                                    LifecycleRequest, CheckpointRequest, RemindRequest, LessonsRequest,
│                                    SessionInfoRequest, ProfileRequest, MemoryRequest,
│                                    MemoryManageRequest, MemorySessionRequest, MemoryAdminFacadeRequest)
├── validation.rs        ~80 lines   MAX_RESULT_LIMIT, MAX_BATCH_SIZE, require_finite(),
│                                    serialize_results(), build_memory_input()
└── tools/
    ├── storage.rs      ~180 lines   memory_store, memory_store_batch, memory_retrieve,
    │                                memory_delete, memory_update bodies
    ├── search.rs       ~200 lines   memory_search, memory_list bodies
    ├── relations.rs    ~120 lines   memory_relations body
    ├── lifecycle.rs    ~160 lines   memory_feedback, memory_lifecycle bodies
    ├── session.rs      ~180 lines   memory_checkpoint, memory_remind, memory_lessons,
    │                                memory_profile, memory_session_info bodies
    └── facades.rs      ~300 lines   memory (unified), memory_manage, memory_session,
                                     memory_admin bodies
```

### 2b. SQLite `mod.rs`: `src/memory_core/storage/sqlite/`

New files extracted from mod.rs; existing submodule files untouched:

```
src/memory_core/storage/sqlite/
├── mod.rs              ~120 lines   Constants (QUERY_CACHE_TTL_SECS, QUERY_CACHE_CAPACITY,
│                                    SUPERSESSION_*), enums (StoreOutcome, InitMode),
│                                    type aliases (QueryCache), submodule declarations,
│                                    pub re-exports
├── storage.rs          ~200 lines   SqliteStorage struct definition, new(), new_default(),
│                                    new_with_path(), with_reranker(), reranker(),
│                                    optimize(), scoring_params(), with_scoring_params(),
│                                    set_scoring_params(), new_query_cache(),
│                                    ensure_vec_extension_registered() [cfg(sqlite-vec)],
│                                    new_in_memory() [cfg(test)], new_in_memory_with_embedder() [cfg(test)],
│                                    test_conn() [cfg(test)], debug_* helpers [cfg(test)]
├── cache.rs            ~180 lines   CachedQuery struct, invalidate_query_cache(),
│                                    invalidate_cache_selective(), cache_entry_could_be_affected(),
│                                    build_stats_paths_json()
├── hot_cache_mgmt.rs   ~120 lines   refresh_hot_cache(), refresh_hot_cache_best_effort(),
│                                    ensure_hot_cache_ready(), start_hot_cache_refresh_task()
├── relationships.rs    ~180 lines   add_relationship(), try_auto_relate(),
│                                    try_create_temporal_edges(), try_create_entity_edges(),
│                                    relationship_exists(), graph_edge_stats()
└── io.rs               ~380 lines   stats(), export_all(), import_all()
                                     (plus RankedSemanticCandidate struct, shared between
                                     mod.rs and advanced.rs — moves here or to a types.rs)
```

> Note: `RankedSemanticCandidate` is defined in `mod.rs` (lines 1225–1243) and used heavily in
> `advanced.rs`. It should move to `storage.rs` or a new `types.rs` so `advanced.rs` can import it.

### 2c. SQLite `advanced.rs`: `src/memory_core/storage/sqlite/pipeline/`

```
src/memory_core/storage/sqlite/pipeline/
├── mod.rs              ~100 lines   Module re-exports, ADVANCED_FTS_CANDIDATE_* constants,
│                                    advanced_fts_candidate_limit()
├── retrieval.rs        ~250 lines   collect_vector_candidates() (Phase 1),
│                                    collect_fts_candidates() (Phase 2)
├── rerank.rs           ~80 lines    compute_cross_encoder_scores() [cfg(real-embeddings)]
├── fusion.rs           ~200 lines   fuse_refine_and_output() Phase 3 (RRF fusion block only):
│                                    RRF scoring, dual-match boost, cross-encoder blend
├── scoring.rs          ~100 lines   refine_scores() (Phase 4)
├── enrichment.rs       ~250 lines   enrich_graph_neighbors() (Phase 5),
│                                    expand_entity_tags() (Phase 5b)
├── abstention.rs       ~130 lines   Abstention + dedup block extracted from fuse_refine_and_output()
│                                    (Phase 6), merge_hot_cache_results(), merge_semantic_result(),
│                                    merge_semantic_metadata()
└── decomp.rs           ~250 lines   run_single_query_pipeline(), query decomposition orchestration
                                     within impl AdvancedSearcher::advanced_search
```

`advanced.rs` residual (renamed to the impl home):

```
src/memory_core/storage/sqlite/advanced.rs  ~350 lines
    impl AdvancedSearcher for SqliteStorage (advanced_search body, cache read/write),
    tests module
```

### 2d. SQLite `admin.rs`: `src/memory_core/storage/sqlite/admin/`

```
src/memory_core/storage/sqlite/admin/
├── mod.rs              ~30 lines    pub use re-exports, constants (MAX_BACKUPS,
│                                    BACKUP_INTERVAL_SECS, BACKUP_PREFIX, BACKUP_SUFFIX)
├── backup.rs           ~310 lines   backups_dir(), collect_backup_entries(),
│                                    create_backup_sync(), rotate_backups_sync(),
│                                    list_backups_sync(), restore_backup_sync(),
│                                    needs_backup(), impl BackupManager for SqliteStorage
├── maintenance.rs      ~580 lines   impl MaintenanceManager for SqliteStorage:
│                                    check_health(), consolidate(), compact(), auto_compact(),
│                                    clear_session(), estimate_tokens()
├── welcome.rs          ~490 lines   impl WelcomeProvider for SqliteStorage:
│                                    welcome(), welcome_scoped()
└── stats.rs            ~220 lines   impl StatsProvider for SqliteStorage:
│                                    type_stats(), session_stats(), weekly_digest(),
│                                    access_rate_stats()
```

---

## 3. Function-to-File Assignments with Line Numbers

### 3a. `src/mcp_server.rs` (2,709 lines)

| Lines | Symbol | Destination |
|-------|--------|-------------|
| 1–25 | imports | `mcp/mod.rs` |
| 27–43 | `MAX_RESULT_LIMIT`, `MAX_BATCH_SIZE`, `require_finite()` | `mcp/validation.rs` |
| 48–117 | `McpToolMode`, `MCP_INSTRUCTIONS` const | `mcp/mod.rs` |
| 119–233 | `ToolMeta`, `TOOL_REGISTRY`, `CATEGORY_ORDER` | `mcp/protocol.rs` |
| 246–266 | `generate_protocol_markdown()` | `mcp/protocol.rs` |
| 270–277 | `tool_registry_json()` | `mcp/protocol.rs` |
| 279–292 | `serialize_results()` | `mcp/validation.rs` |
| 294–329 | `build_memory_input()` | `mcp/validation.rs` |
| 331–357 | `McpMemoryServer` struct, `new()`, `with_tool_mode()`, `serve_stdio()` | `mcp/mod.rs` |
| 361–699 | All `*Request` structs (15 named + 4 unified = 19 total) | `mcp/request_types.rs` |
| 702–755 | `memory_store`, `memory_store_batch` tool bodies | `mcp/tools/storage.rs` |
| 757–971 | `memory_retrieve`, `memory_search` tool bodies | `mcp/tools/search.rs` (search); `mcp/tools/storage.rs` (retrieve) |
| 973–1030 | `memory_list` tool body | `mcp/tools/search.rs` |
| 1032–1087 | `memory_delete`, `memory_update` tool bodies | `mcp/tools/storage.rs` |
| 1089–1233 | `memory_relations` tool body | `mcp/tools/relations.rs` |
| 1236–1261 | `memory_feedback` tool body | `mcp/tools/lifecycle.rs` |
| 1263–1423 | `memory_lifecycle` tool body | `mcp/tools/lifecycle.rs` |
| 1425–1523 | `memory_checkpoint` tool body | `mcp/tools/session.rs` |
| 1525–1617 | `memory_remind`, `memory_lessons` tool bodies | `mcp/tools/session.rs` |
| 1619–1688 | `memory_profile`, `memory_session_info` tool bodies | `mcp/tools/session.rs` |
| 1690–1780 | `memory` unified facade body | `mcp/tools/facades.rs` |
| 1782–2201 | `memory_manage` unified facade body | `mcp/tools/facades.rs` |
| 2203–2627 | `memory_session`, `memory_admin` unified facade bodies | `mcp/tools/facades.rs` |
| 2630–2638 | `ServerHandler` impl (`get_info`) | `mcp/mod.rs` |
| 2641–2709 | tests | `mcp/mod.rs` (tests submodule) |

**`#[tool_router]` constraint:** The `#[tool_router]` proc-macro requires all `#[tool(...)]`
methods to be in a single `impl McpMemoryServer` block. The solution is a **thin-wrapper
pattern**: `mcp/mod.rs` keeps the `#[tool_router] impl McpMemoryServer` block with 1-3 line
wrapper methods that delegate to functions in `tools/*.rs`. The tool function bodies live in the
`tools/` files as free functions or methods on a helper struct, and are called via `use
crate::mcp::tools::storage::memory_store_impl` etc. Each wrapper is:

```rust
async fn memory_store(&self, params: Parameters<StoreRequest>) -> Result<CallToolResult, McpError> {
    tools::storage::memory_store_impl(&self.storage, params).await
}
```

### 3b. `src/memory_core/storage/sqlite/mod.rs` (1,281 lines)

| Lines | Symbol | Destination |
|-------|--------|-------------|
| 1–28 | imports | `mod.rs` (residual) |
| 29–51 | `QUERY_CACHE_TTL_SECS`, `QUERY_CACHE_CAPACITY`, `CachedQuery`, `QueryCache` | `cache.rs` |
| 53–62 | `SUPERSESSION_COSINE_THRESHOLD`, `SUPERSESSION_JACCARD_THRESHOLD` | `mod.rs` or `storage.rs` |
| 63–73 | `StoreOutcome`, `InitMode` enums | `mod.rs` (shared types) |
| 81–95 | `SqliteStorage` struct | `storage.rs` |
| 97–119 | `ensure_vec_extension_registered()`, `new_query_cache()` | `storage.rs` |
| 121–222 | `impl SqliteStorage { new(), new_default(), new_with_path(), with_reranker(), reranker(), optimize(), scoring_params(), with_scoring_params(), set_scoring_params() }` | `storage.rs` |
| 224–306 | `invalidate_query_cache()`, `invalidate_cache_selective()`, `cache_entry_could_be_affected()` | `cache.rs` |
| 308–411 | `refresh_hot_cache()`, `refresh_hot_cache_best_effort()`, `ensure_hot_cache_ready()`, `start_hot_cache_refresh_task()` | `hot_cache_mgmt.rs` |
| 413–451 | `add_relationship()` | `relationships.rs` |
| 453–461 | `store()` and `update()` forwarding methods | `storage.rs` |
| 463–535 | `graph_edge_stats()`, `stats()` | `relationships.rs` (`graph_edge_stats`); `io.rs` (`stats`) |
| 537–660 | `export_all()` | `io.rs` |
| 662–824 | `import_all()` | `io.rs` |
| 826–902 | `new_in_memory()`, `new_in_memory_with_embedder()`, `test_conn()`, `debug_*` helpers [cfg(test)] | `storage.rs` |
| 954–1059 | `try_auto_relate()` | `relationships.rs` |
| 1061–1097 | `try_create_temporal_edges()` | `relationships.rs` |
| 1099–1177 | `try_create_entity_edges()` | `relationships.rs` |
| 1179–1206 | `relationship_exists()` | `relationships.rs` |
| 1209–1223 | `build_stats_paths_json()` | `cache.rs` or `io.rs` |
| 1225–1243 | `RankedSemanticCandidate` struct | `storage.rs` (or new `types.rs`) |
| 1245–1281 | `mod` declarations, `use` re-exports | `mod.rs` (residual) |

### 3c. `src/memory_core/storage/sqlite/advanced.rs` (1,739 lines)

| Lines | Symbol | Destination |
|-------|--------|-------------|
| 1–23 | imports, `ADVANCED_FTS_*` constants, `advanced_fts_candidate_limit()` | `pipeline/mod.rs` |
| 25–190 | `collect_vector_candidates()` (Phase 1) | `pipeline/retrieval.rs` |
| 192–313 | `collect_fts_candidates()` (Phase 2) | `pipeline/retrieval.rs` |
| 315–366 | `compute_cross_encoder_scores()` [cfg(real-embeddings)] | `pipeline/rerank.rs` |
| 368–448 | `refine_scores()` (Phase 4) | `pipeline/scoring.rs` |
| 450–672 | `enrich_graph_neighbors()` (Phase 5) | `pipeline/enrichment.rs` |
| 674–866 | `expand_entity_tags()` (Phase 5b) | `pipeline/enrichment.rs` |
| 868–1093 | `fuse_refine_and_output()` — Phase 3 RRF block (868–999) + abstention/dedup (1035–1093) | `pipeline/fusion.rs` (RRF), `pipeline/abstention.rs` (dedup+gate) |
| 1095–1170 | `merge_hot_cache_results()`, `merge_semantic_result()`, `merge_semantic_metadata()` | `pipeline/abstention.rs` |
| 1172–1293 | `run_single_query_pipeline()` | `pipeline/decomp.rs` |
| 1295–1613 | `impl AdvancedSearcher for SqliteStorage { advanced_search() }` | `advanced.rs` (residual, calls pipeline functions) |
| 1615–1739 | tests | `advanced.rs` (residual, tests submodule) |

### 3d. `src/memory_core/storage/sqlite/admin.rs` (1,619 lines)

| Lines | Symbol | Destination |
|-------|--------|-------------|
| 1–12 | imports, `MAX_BACKUPS`, `BACKUP_INTERVAL_SECS`, `BACKUP_PREFIX`, `BACKUP_SUFFIX` | `admin/mod.rs` |
| 14–40 | `backups_dir()`, `collect_backup_entries()` | `admin/backup.rs` |
| 42–157 | `create_backup_sync()`, `rotate_backups_sync()`, `list_backups_sync()`, `restore_backup_sync()` | `admin/backup.rs` |
| 220–311 | `needs_backup()`, `impl BackupManager for SqliteStorage` | `admin/backup.rs` |
| 313–886 | `impl MaintenanceManager for SqliteStorage` (check_health, consolidate, compact, auto_compact, clear_session, estimate_tokens) | `admin/maintenance.rs` |
| 887–1379 | `impl WelcomeProvider for SqliteStorage` (welcome, welcome_scoped) | `admin/welcome.rs` |
| 1381–1619 | `impl StatsProvider for SqliteStorage` (type_stats, session_stats, weekly_digest, access_rate_stats) | `admin/stats.rs` |

---

## 4. File Size Targets

| Target File | Estimated Lines | Status |
|-------------|----------------|--------|
| `mcp/mod.rs` | ~400 | Within limit |
| `mcp/protocol.rs` | ~200 | Within limit |
| `mcp/request_types.rs` | ~350 | Within limit |
| `mcp/validation.rs` | ~80 | Within limit |
| `mcp/tools/storage.rs` | ~180 | Within limit |
| `mcp/tools/search.rs` | ~200 | Within limit |
| `mcp/tools/relations.rs` | ~120 | Within limit |
| `mcp/tools/lifecycle.rs` | ~160 | Within limit |
| `mcp/tools/session.rs` | ~180 | Within limit |
| `mcp/tools/facades.rs` | ~300 | Within limit |
| `sqlite/mod.rs` (residual) | ~120 | Within limit |
| `sqlite/storage.rs` | ~200 | Within limit |
| `sqlite/cache.rs` | ~180 | Within limit |
| `sqlite/hot_cache_mgmt.rs` | ~120 | Within limit |
| `sqlite/relationships.rs` | ~180 | Within limit |
| `sqlite/io.rs` | ~380 | Within limit |
| `pipeline/mod.rs` | ~100 | Within limit |
| `pipeline/retrieval.rs` | ~250 | Within limit |
| `pipeline/rerank.rs` | ~80 | Within limit |
| `pipeline/fusion.rs` | ~200 | Within limit |
| `pipeline/scoring.rs` | ~100 | Within limit |
| `pipeline/enrichment.rs` | ~250 | Within limit |
| `pipeline/abstention.rs` | ~130 | Within limit |
| `pipeline/decomp.rs` | ~250 | Within limit |
| `advanced.rs` (residual) | ~350 | Within limit |
| `admin/mod.rs` | ~30 | Within limit |
| `admin/backup.rs` | ~310 | Within limit |
| `admin/maintenance.rs` | ~580 | Exception justified |
| `admin/welcome.rs` | ~490 | Exception justified |
| `admin/stats.rs` | ~220 | Within limit |

**Exception justifications:**

- `admin/maintenance.rs` (~580 lines): `MaintenanceManager` has 5 methods (`check_health`,
  `consolidate`, `compact`, `auto_compact`, `clear_session`) that share the `estimate_tokens()`
  helper and have strong semantic cohesion as a single trait impl. Splitting individual methods
  into sub-files would create excessive file-per-method granularity with no navigability benefit.

- `admin/welcome.rs` (~490 lines): `welcome()` and `welcome_scoped()` are tightly coupled — they
  share identical JSON output shape and `welcome_scoped()` delegates to `welcome()` for the
  simple case. Separation would require either duplication or continued cross-file calling.

---

## 5. Module Dependency Graph

```
src/main.rs
  └── mcp/mod.rs
        ├── mcp/protocol.rs           (no deps on other mcp/ files)
        ├── mcp/request_types.rs      (no deps on other mcp/ files)
        ├── mcp/validation.rs         ← mcp/request_types.rs
        └── mcp/tools/
              ├── storage.rs          ← mcp/request_types.rs, mcp/validation.rs
              ├── search.rs           ← mcp/request_types.rs, mcp/validation.rs
              ├── relations.rs        ← mcp/request_types.rs, mcp/validation.rs
              ├── lifecycle.rs        ← mcp/request_types.rs, mcp/validation.rs
              ├── session.rs          ← mcp/request_types.rs, mcp/validation.rs
              └── facades.rs          ← mcp/request_types.rs, mcp/validation.rs,
                                         + all other tools/* (delegates into them)
        All tools/* → memory_core::storage::SqliteStorage (via trait dispatch)

src/memory_core/storage/sqlite/
  mod.rs
    ├── storage.rs         (SqliteStorage + constructors)
    ├── cache.rs           ← storage.rs (CachedQuery lives here, SqliteStorage methods)
    ├── hot_cache_mgmt.rs  ← storage.rs, hot_cache.rs (existing)
    ├── relationships.rs   ← storage.rs, conn_pool.rs
    ├── io.rs              ← storage.rs, conn_pool.rs, helpers.rs
    ├── advanced.rs        ← pipeline/ (calls all pipeline phase functions)
    │     └── pipeline/
    │           ├── retrieval.rs   ← helpers.rs, embedding_codec.rs
    │           ├── rerank.rs      ← reranker (real-embeddings feature)
    │           ├── fusion.rs      ← retrieval.rs (types), scoring.rs
    │           ├── scoring.rs     ← memory_core::scoring
    │           ├── enrichment.rs  ← helpers.rs, memory_core::domain
    │           ├── abstention.rs  ← helpers.rs
    │           └── decomp.rs      ← retrieval.rs, fusion.rs, abstention.rs
    └── admin/
          ├── backup.rs      ← conn_pool.rs
          ├── maintenance.rs ← conn_pool.rs, helpers.rs, embedding_codec.rs
          ├── welcome.rs     ← advanced.rs (calls AdvancedSearcher), session.rs (ProfileManager, ReminderManager)
          └── stats.rs       ← conn_pool.rs
```

### Circular Dependency Analysis

**No circular dependencies exist or will be introduced**, with one caveat to watch:

- `admin/welcome.rs` calls `<SqliteStorage as AdvancedSearcher>::advanced_search()`, which is
  implemented in `advanced.rs`. Both are submodules of the same `sqlite/` crate, so this is a
  sibling-module call, not a circular import. The call goes through the trait object, not a direct
  module path. This pattern is already present today and is safe.

- `mcp/tools/facades.rs` delegates to the other `tools/*.rs` files. This is a fan-out pattern,
  not a cycle. `facades.rs` imports from sibling `tools/` modules; none of those siblings import
  from `facades.rs`.

- `pipeline/decomp.rs` calls `run_single_query_pipeline()` which calls `collect_vector_candidates`,
  `collect_fts_candidates`, and `fuse_refine_and_output()` — all within `pipeline/`. No cycle.

---

## 6. Migration Strategy

### Phase Sequence

```
Phase 1: admin.rs → admin/          (independent, easiest, 4 clean groups)
Phase 2: mod.rs → 5 new files       (touches more code but no trait logic changes)
Phase 3: advanced.rs → pipeline/    (requires benchmark gate — search quality sensitive)
Phase 4: mcp_server.rs → mcp/       (independent of phases 1-3, can run in parallel)
```

### PR Boundaries

Each PR must be a pure refactor: zero behavioral changes, zero test additions or removals.
All PRs must pass `prek run` and, for phases 2-3, `./scripts/bench.sh --gate`.

**PR 1: `refactor(sqlite): split admin.rs into admin/ subdir`**
- Create `src/memory_core/storage/sqlite/admin/` directory
- Move `impl BackupManager` → `admin/backup.rs`
- Move `impl MaintenanceManager` → `admin/maintenance.rs`
- Move `impl WelcomeProvider` → `admin/welcome.rs`
- Move `impl StatsProvider` → `admin/stats.rs`
- Replace `admin.rs` with `admin/mod.rs` re-exporting `use super::*` (so `mod.rs` declarations are unchanged)
- Test gate: `prek run` only (admin logic is not on the search path)

**PR 2: `refactor(sqlite): decompose mod.rs into storage/cache/relationships/io`**
- Create `storage.rs` (struct + constructors + test helpers)
- Create `cache.rs` (CachedQuery + invalidation methods + `build_stats_paths_json`)
- Create `hot_cache_mgmt.rs` (background task methods)
- Create `relationships.rs` (add_relationship + auto-relate methods)
- Create `io.rs` (stats + export_all + import_all)
- Move `RankedSemanticCandidate` to `storage.rs` (or `types.rs` if cleaner)
- `mod.rs` becomes ~120 lines of constants, type aliases, submodule declarations, re-exports
- Test gate: `prek run` + `./scripts/bench.sh --gate`

**PR 3: `refactor(sqlite): extract search pipeline into pipeline/ subdir`**
- Create `src/memory_core/storage/sqlite/pipeline/`
- Move phase functions per section 3c
- `advanced.rs` residual keeps `impl AdvancedSearcher` + tests
- This is the highest-risk PR: touches the search scoring path directly
- Test gate: `prek run` + `./scripts/bench.sh --gate`; if gate warns, run `--samples 10` validation

**PR 4: `refactor(mcp): split mcp_server.rs into mcp/ module`**
- Create `src/mcp/` directory
- Apply thin-wrapper pattern for `#[tool_router]` constraint
- Move request types, validation, protocol, tool bodies
- Update `src/main.rs` import path from `mcp_server` to `mcp`
- Test gate: `prek run` (MCP tools are integration-tested via `tests/mcp_smoke.rs`)

PRs 1 and 4 are **fully independent** and can be done in parallel. PRs 2 and 3 depend on
each other only in that PR 3 will be easier after PR 2 (no conflicts on `mod.rs`).

### Test Gates by Phase

| Phase | Gate |
|-------|------|
| PR 1 (admin) | `prek run` |
| PR 2 (mod.rs) | `prek run` + `./scripts/bench.sh --gate` |
| PR 3 (pipeline) | `prek run` + `./scripts/bench.sh --gate` + full `--samples 10` if gate warns |
| PR 4 (mcp) | `prek run` |

---

## 7. What Moves vs. What Stays

### What Moves (extracted from god modules)

- **From `mcp_server.rs`**: All `*Request` structs, all tool function bodies, `TOOL_REGISTRY`,
  `MCP_INSTRUCTIONS`, `generate_protocol_markdown()`, `tool_registry_json()`, `ToolMeta`,
  `serialize_results()`, `build_memory_input()`, `require_finite()`
- **From `sqlite/mod.rs`**: `SqliteStorage` struct, all constructors and scoring methods,
  cache management methods, hot-cache background task, relationship operations, bulk I/O methods,
  `RankedSemanticCandidate` struct, `build_stats_paths_json()`
- **From `sqlite/advanced.rs`**: All 7 phase functions (Phase 1 through Phase 6),
  `merge_hot_cache_results()`, `merge_semantic_result()`, `merge_semantic_metadata()`,
  `run_single_query_pipeline()`
- **From `sqlite/admin.rs`**: All four `impl` blocks (BackupManager, MaintenanceManager,
  WelcomeProvider, StatsProvider) plus all sync helper functions

### What Stays In Place (unchanged)

- All existing `sqlite/` submodules: `crud.rs`, `search.rs`, `graph.rs`, `session.rs`,
  `lifecycle.rs`, `helpers.rs`, `nlp.rs`, `query_classifier.rs`, `temporal.rs`,
  `conn_pool.rs`, `embedding_codec.rs`, `hot_cache.rs`, `entities.rs`, `schema.rs`
- All `memory_core/` trait definitions (`traits.rs`, `domain.rs`, `scoring.rs`)
- `src/main.rs` (only the import path for mcp_server changes)
- Test files in `src/memory_core/storage/sqlite/tests/` (unchanged)
- `tests/mcp_smoke.rs` (unchanged)
- All benchmark code under `benches/`

### Visibility Changes Required

Functions currently private to `sqlite/mod.rs` that are called by submodules will need
`pub(super)` or `pub(crate)` visibility when moved:

- `invalidate_query_cache()` and `invalidate_cache_selective()` — already `pub(super)`;
  move to `cache.rs`, visibility unchanged
- `refresh_hot_cache()`, `refresh_hot_cache_best_effort()`, `ensure_hot_cache_ready()` —
  already `pub(super)`; move to `hot_cache_mgmt.rs`, re-export via `mod.rs`
- `try_auto_relate()`, `try_create_temporal_edges()`, `try_create_entity_edges()` — private;
  called only from `crud.rs` (via `impl Storage`); need `pub(super)` in `relationships.rs`

---

## 8. Build Sequence Checklist

Use this checklist for each PR. Run items in order; stop and fix before proceeding.

### Pre-work (all phases)
- [ ] Confirm working on a fresh branch from current `main`
- [ ] Run `prek run` baseline — must pass clean before any changes

### Phase 1 — admin/
- [ ] Create `src/memory_core/storage/sqlite/admin/` directory
- [ ] Create `admin/backup.rs` — copy BackupManager impl + sync helpers
- [ ] Create `admin/maintenance.rs` — copy MaintenanceManager impl + `estimate_tokens`
- [ ] Create `admin/welcome.rs` — copy WelcomeProvider impl
- [ ] Create `admin/stats.rs` — copy StatsProvider impl
- [ ] Create `admin/mod.rs` — constants + `mod backup; mod maintenance; mod welcome; mod stats;` + re-exports
- [ ] Replace `admin.rs` with `admin/mod.rs` (jj tracks this as a delete + creates)
- [ ] Update `sqlite/mod.rs` `mod admin;` declaration (unchanged if `mod.rs` approach used)
- [ ] `cargo build --all-features` — must compile
- [ ] `prek run` — must pass
- [ ] Submit PR

### Phase 2 — mod.rs decomposition
- [ ] Create `sqlite/storage.rs` — SqliteStorage struct + all constructors + test helpers
- [ ] Create `sqlite/cache.rs` — CachedQuery, QueryCache, cache methods, build_stats_paths_json
- [ ] Create `sqlite/hot_cache_mgmt.rs` — four hot-cache methods
- [ ] Create `sqlite/relationships.rs` — relationship methods
- [ ] Create `sqlite/io.rs` — stats, export_all, import_all
- [ ] Move `RankedSemanticCandidate` to `storage.rs` — update import in `advanced.rs`
- [ ] Rewrite `sqlite/mod.rs` as ~120 line shell with submodule decls + re-exports
- [ ] Verify all `pub(super)` visibility is correct for cross-submodule calls
- [ ] `cargo build --all-features` — must compile
- [ ] `prek run` — must pass
- [ ] `./scripts/bench.sh --gate` — must pass (warn < 2pp, fail < 5pp delta)
- [ ] Submit PR

### Phase 3 — pipeline/ extraction (benchmark-gated)
- [ ] Create `src/memory_core/storage/sqlite/pipeline/` directory
- [ ] Create `pipeline/retrieval.rs` — Phase 1 + Phase 2 functions
- [ ] Create `pipeline/rerank.rs` — cross-encoder function
- [ ] Create `pipeline/fusion.rs` — RRF block from fuse_refine_and_output
- [ ] Create `pipeline/scoring.rs` — refine_scores
- [ ] Create `pipeline/enrichment.rs` — enrich_graph_neighbors + expand_entity_tags
- [ ] Create `pipeline/abstention.rs` — abstention/dedup block + merge helpers
- [ ] Create `pipeline/decomp.rs` — run_single_query_pipeline + query decomp logic
- [ ] Create `pipeline/mod.rs` — constants + re-exports
- [ ] Rewrite `advanced.rs` as residual — `impl AdvancedSearcher` calling pipeline functions
- [ ] Add `mod pipeline;` to `sqlite/mod.rs`
- [ ] `cargo build --all-features` — must compile
- [ ] `cargo build --no-default-features` — verify no missing cfg gates
- [ ] `prek run` — must pass
- [ ] `./scripts/bench.sh --gate` — must pass
- [ ] If bench gate warns: `./scripts/bench.sh --samples 10 --notes "pre-merge pipeline refactor"` — must pass
- [ ] Submit PR

### Phase 4 — mcp/ extraction (independent)
- [ ] Create `src/mcp/` directory and `src/mcp/tools/` directory
- [ ] Create `mcp/request_types.rs` — all Request structs
- [ ] Create `mcp/validation.rs` — constants + validation functions
- [ ] Create `mcp/protocol.rs` — TOOL_REGISTRY + protocol functions
- [ ] Create `mcp/tools/storage.rs` — storage tool impl functions
- [ ] Create `mcp/tools/search.rs` — search/list tool impl functions
- [ ] Create `mcp/tools/relations.rs` — relations tool impl function
- [ ] Create `mcp/tools/lifecycle.rs` — feedback/lifecycle tool impl functions
- [ ] Create `mcp/tools/session.rs` — checkpoint/remind/lessons/profile/session_info impl functions
- [ ] Create `mcp/tools/facades.rs` — unified facade impl functions
- [ ] Rewrite `mcp/mod.rs` — McpMemoryServer, thin `#[tool_router]` wrappers, ServerHandler impl
- [ ] Update `src/lib.rs` or `src/main.rs` — change `mod mcp_server;` to `mod mcp;`
- [ ] `cargo build --all-features` — must compile
- [ ] `prek run` — must pass
- [ ] Run `tests/mcp_smoke.rs` explicitly: `cargo test --all-features mcp_smoke`
- [ ] Submit PR

---

## 9. Key Invariants to Preserve

1. **`#[tool_router]` constraint**: The proc-macro requires a single `impl McpMemoryServer` block
   containing all `#[tool(...)]` methods. Thin wrappers of 1-3 lines satisfy this while keeping
   actual logic in `tools/*.rs`.

2. **Benchmark regression gate**: Any change to `advanced.rs` or its pipeline dependencies must
   pass `./scripts/bench.sh --gate`. The LoCoMo-10 baseline is 90.1% (word-overlap scoring).
   A >5pp drop fails the gate; 2-5pp triggers a full 10-sample validation run.

3. **No behavioral changes**: Each PR must be a pure mechanical refactor. No logic changes,
   parameter renames, or algorithm tweaks. Verify with `git diff` showing only file movements.

4. **`spawn_blocking` pattern**: All SQLite I/O must remain wrapped in `tokio::task::spawn_blocking`.
   Moving functions between files does not change this requirement.

5. **`pub(super)` visibility**: Functions crossing the new module boundaries need explicit
   `pub(super)` or `pub(crate)`. Audit each moved function for callers in sibling modules.

6. **Feature flag guards**: `#[cfg(feature = "real-embeddings")]` and `#[cfg(feature = "sqlite-vec")]`
   gates must be preserved exactly as-is on moved code. Test with `--no-default-features`.
