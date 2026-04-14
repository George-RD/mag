# MAG Rust Source Code Structure — Complete Recon Map

**Generated**: 2026-04-14  
**Scout**: Uruk-hai Agent  
**Stronghold Path**: `/Users/george/repos/mag/docs/strongholds/recon-source-tree.md`

---

## Executive Summary

The MAG codebase comprises **41 Rust source files** across two main domains:
- **Domain Layer**: Memory system abstractions (traits, domain models)
- **Storage/Backend Layer**: SQLite-backed implementation with pluggable embeddings
- **CLI/Server Layer**: Command-line interface and MCP protocol server

**Key Findings**:
- **9 god modules** identified (>500 lines) — primarily in sqlite storage subsystem
- **28 public traits** — core abstractions for memory operations
- **22 core domain structs** — data models and pipeline configurations
- **Module hierarchy**: Well-separated concerns (domain → traits → storage → sqlite)
- **Concerns Analysis**: SQLite module handles too many responsibilities (CRUD, graph ops, lifecycle, NLP)

---

## I. File Inventory with Line Counts

### Root Level (9 files)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `lib.rs` | 25 | Module re-exports, in-memory test helpers | Metadata |
| `main.rs` | 1,725 | CLI command routing, daemon HTTP, MCP stdio server | **GOD MODULE** |
| `mcp_server.rs` | 2,709 | MCP protocol handler with 19 tools | **GOD MODULE** |
| `cli.rs` | 1,255 | CLI argument parsing and command dispatch | **GOD MODULE** |
| `setup.rs` | 1,844 | Interactive setup wizard and config generation | **GOD MODULE** |
| `config_writer.rs` | 1,844 | Configuration file generation and defaults | **GOD MODULE** |
| `uninstall.rs` | 1,382 | Uninstall routines and cleanup | **GOD MODULE** |
| `tool_detection.rs` | 1,395 | Tool invocation analysis and detection | **GOD MODULE** |
| `benchmarking.rs` | 372 | Benchmark harness and metadata | Standard |

### Authentication & Infrastructure (4 files)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `auth.rs` | 243 | Tower auth middleware for HTTP | Standard |
| `idle_timer.rs` | 143 | Idle timeout tracking for daemon | Standard |
| `app_paths.rs` | 176 | XDG-compliant path resolution | Standard |
| `daemon.rs` | 283 | Daemon HTTP server setup | Standard |

### Test Utilities (1 file)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `test_helpers.rs` | 75 | Mock implementations for testing | Test Helper |

### Binaries (1 file)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `bin/fetch_benchmark_data.rs` | 63 | Standalone benchmark data fetcher | Utility Binary |

### Memory Core Domain (3 files)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `memory_core/mod.rs` | 419 | Pipeline orchestrator + module re-exports | Standard |
| `memory_core/domain.rs` | 587 | Domain types: MemoryInput, SearchResult, EventType | Standard |
| `memory_core/traits.rs` | 285 | 28 public traits defining storage interface | Standard |

### Memory Core Processing (3 files)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `memory_core/embedder.rs` | 967 | Placeholder + ONNX embedder implementations | Standard |
| `memory_core/reranker.rs` | 384 | Cross-encoder reranking for search results | Standard |
| `memory_core/scoring.rs` | 1,216 | Semantic search scoring, decay, and weighting | **GOD MODULE** |

### Storage Layer (1 file, metadata module)

| File | Lines | Purpose | Category |
|------|-------|---------|----------|
| `memory_core/storage/mod.rs` | 4 | Re-exports sqlite submodule | Metadata |

### SQLite Backend (15 files, 18,251 lines total)

| File | Lines | Purpose | Category | Concern Count |
|------|-------|---------|----------|---|
| `sqlite/mod.rs` | 1,281 | Primary struct + trait impls | **GOD MODULE** | 5+ |
| `sqlite/tests.rs` | 7,632 | Integration test suite | Test Suite | - |
| `sqlite/schema.rs` | 427 | DDL definitions and migrations | Standard | 1 |
| `sqlite/entities.rs` | 482 | Row-to-struct mapping | Standard | 1 |
| `sqlite/embedding_codec.rs` | 62 | Vector serialization for sqlite-vec | Standard | 1 |
| `sqlite/conn_pool.rs` | 336 | Connection pooling (WAL mode) | Standard | 1 |
| `sqlite/crud.rs` | 809 | Create/read/update/delete operations | **GOD MODULE** | 3+ |
| `sqlite/session.rs` | 601 | Session lifecycle and summary | Standard | 2 |
| `sqlite/helpers.rs` | 1,237 | Scoring helpers and utilities | **GOD MODULE** | 3+ |
| `sqlite/hot_cache.rs` | 276 | LRU in-memory cache for frequent queries | Standard | 1 |
| `sqlite/lifecycle.rs` | 271 | Memory expiration and housekeeping | Standard | 2 |
| `sqlite/temporal.rs` | 280 | Time-based querying and decay | Standard | 1 |
| `sqlite/search.rs` | 423 | FTS and full-text search | Standard | 1 |
| `sqlite/nlp.rs` | 547 | Lemmatization, stemming, tokenization | Standard | 1 |
| `sqlite/query_classifier.rs` | 450 | Query type detection and routing | Standard | 1 |
| `sqlite/admin.rs` | 1,619 | Health checks, backup, consolidation | **GOD MODULE** | 4+ |
| `sqlite/advanced.rs` | 1,739 | Compaction, deduplication, graph ops | **GOD MODULE** | 4+ |
| `sqlite/graph.rs` | 532 | Relationship graph operations | Standard | 1 |

---

## II. God Modules (>500 lines)

Files exhibiting multiple unrelated concerns and candidates for refactoring:

### Tier 1: Critical Refactoring Needed (>1500 lines)

1. **`mcp_server.rs` (2,709 lines)**
   - Concerns: MCP protocol marshaling, 19 tool definitions, error handling, parameter validation
   - Should split: `mcp_protocol.rs`, `mcp_tools/{batch.rs, query.rs, manage.rs, admin.rs}`

2. **`sqlite/mod.rs` (1,281 lines)**
   - Concerns: Connection pool management, query caching, trait impls (Storage, Retriever, Searcher, Deleter, Updater, Tagger, Lister, RelationshipQuerier, VersionChainQuerier, FeedbackRecorder, ExpirationSweeper, ProfileManager, CheckpointManager, ReminderManager, LessonQuerier, MaintenanceManager, WelcomeProvider, BackupManager, StatsProvider), hot cache invalidation
   - **SEVERE**: Implements 19 public traits; should delegate to submodules per concern
   - Should split: Keep only initialization and dispatch logic; move trait impls to respective submodules

3. **`setup.rs` (1,844 lines)** / **`config_writer.rs` (1,844 lines)**
   - Concerns: Interactive prompts, file I/O, config generation, validation
   - Could merge these or split each into `prompts.rs` + `writer.rs`

4. **`sqlite/admin.rs` (1,619 lines)**
   - Concerns: Health checks, backup/restore, consolidation, rotation, compaction coordination
   - Should split: `backup_manager.rs`, `health_check.rs`, `consolidation.rs`

5. **`sqlite/advanced.rs` (1,739 lines)**
   - Concerns: Compaction algorithm, graph traversal, deduplication, feedback accumulation
   - Should split: `compaction.rs`, `graph_operations.rs`, `feedback.rs`

### Tier 2: Moderate Refactoring (1000-1500 lines)

6. **`main.rs` (1,725 lines)**
   - Concerns: Tokio runtime setup, daemon lifecycle, MCP dispatcher, CLI fallback routing
   - Should split: `daemon_http.rs`, `mcp_dispatcher.rs`, separate CLI into subcommands

7. **`cli.rs` (1,255 lines)**
   - Concerns: Argument parsing, command dispatch, output formatting, error messages
   - Should split: `commands/{store.rs, query.rs, manage.rs, admin.rs}`, `output.rs`, `errors.rs`

8. **`tool_detection.rs` (1,395 lines)**
   - Concerns: Stack parsing, regex-based detection, tool classification, result formatting
   - Should split: `parser.rs`, `detector.rs`, `classifier.rs`

9. **`uninstall.rs` (1,382 lines)**
   - Concerns: Interactive confirmation, file deletion, config cleanup, logging
   - Should split: `remover.rs`, `prompts.rs`, `validators.rs`

10. **`memory_core/scoring.rs` (1,216 lines)**
    - Concerns: Scoring functions, decay curves, weight calculations, helper algorithms
    - Should split: `semantic_scoring.rs`, `temporal_decay.rs`, `text_processing.rs`, `weighting.rs`

### Tier 3: Minor Refactoring (600-1000 lines)

11. **`sqlite/helpers.rs` (1,237 lines)**
    - Concerns: Jaccard similarity, token overlap, embedding comparison, keyword extraction
    - Should split: `similarity_metrics.rs`, `text_analysis.rs`, `keyword_extraction.rs`

12. **`sqlite/crud.rs` (809 lines)**
    - Concerns: Insert logic, bulk operations, update with auto-relations, delete cascades
    - Could stay unified if kept focused on CRUD only (currently +relations logic)

### Tier 4: Acceptable Size (500-600 lines)

13. **`sqlite/session.rs` (601 lines)**
    - Concerns: Session creation, summary generation, active session tracking
    - Well-scoped — acceptable as-is

14. **`sqlite/nlp.rs` (547 lines)**
    - Concerns: Stopword lists, stemming, lemmatization, tokenization
    - Well-scoped — acceptable as-is

---

## III. Public Trait Definitions (28 total)

All traits located in: `/Users/george/repos/mag/src/memory_core/traits.rs`

### CRUD Traits (5)
- `Storage` — Store memory with MemoryInput
- `Retriever` — Fetch memory by ID
- `Deleter` — Delete by ID → bool
- `Updater` — Partial updates via MemoryUpdate
- `Lister` — Paginated list with count

### Search Traits (7)
- `Searcher` — Keyword/FTS search
- `SemanticSearcher` — Embedding-based similarity search
- `Recents` — Recent memories by timestamp
- `PhraseSearcher` — Exact phrase matching
- `AdvancedSearcher` — Combined scoring (semantic + keyword)
- `SimilarFinder` — Find similar by memory_id
- `Tagger` — Query by tags (AND logic)

### Graph/Relationship Traits (3)
- `GraphTraverser` — Multi-hop relationship traversal
- `RelationshipQuerier` — Get all relationships for a memory
- `VersionChainQuerier` — Track version supersession chains

### Metadata Traits (8)
- `ProfileManager` — User profile storage
- `CheckpointManager` — Task checkpoints and resume
- `ReminderManager` — Reminder CRUD and dismiss
- `LessonQuerier` — Query lessons by context
- `ExpirationSweeper` — TTL cleanup
- `FeedbackRecorder` — Feedback collection
- `MaintenanceManager` — Health, consolidate, compact, clear_session, auto_compact
- `WelcomeProvider` — Session briefing generation

### Admin Traits (2)
- `BackupManager` — Create, rotate, restore, list backups
- `StatsProvider` — Type/session/weekly/access stats

### Embedder Trait (1)
- `Embedder` (in `embedder.rs`) — Embed text → Vec<f32>

### Legacy Trait Stubs (2)
- `Ingestor` — Content ingestion (placeholder)
- `Processor` — Content processing (placeholder)

---

## IV. Core Domain Structs (Domain Models)

Located in: `/Users/george/repos/mag/src/memory_core/domain.rs`

### Input/Configuration
- `MemoryInput` — Memory with tags, importance, event_type, TTL, session/project context
- `MemoryUpdate` — Partial update with optional fields
- `SearchOptions` — Query filters (event_type, project, session_id, date range, importance min, tags)
- `WelcomeOptions` — Session briefing configuration
- `CheckpointInput` — Task checkpoint metadata

### Domain Types
- `EventType` (enum) — 22 event types (SessionSummary, Decision, ErrorPattern, ..., Unknown)
- `MemoryKind` (enum) — Episodic vs Semantic classification

### Output/Result Types
- `SearchResult` — Memory + tags + importance + metadata (no score)
- `SemanticResult` — SearchResult + similarity_score (f32)
- `ListResult` — Paginated: Vec<SearchResult> + total_count
- `GraphNode` — Hop-numbered relationship with weight and edge_type
- `Relationship` — Directed edge: source → target with rel_type and metadata
- `BackupInfo` — Backup path, size_bytes, created_at

### Constants
- TTL enums: `TTL_EPHEMERAL` (1h), `TTL_SHORT_TERM` (1d), `TTL_LONG_TERM` (14d)
- Relationship types: `REL_PRECEDED_BY`, `REL_RELATES_TO`, `REL_SIMILAR_TO`, `REL_SHARES_THEME`, `REL_PARALLEL_CONTEXT`

---

## V. Storage Implementation Structs

Located in: `/Users/george/repos/mag/src/memory_core/storage/sqlite/`

### Main Implementation
- `SqliteStorage` — 19 trait implementations, connection pool, query cache, hot cache

### Supporting Structs
- `ConnPool` — WAL-mode connection pooling (1 writer + N readers)
- `HotTierCache` — LRU cache for frequent queries
- `CachedQuery` — Query results + filter metadata
- `ScoringParams` — Weighting configuration for search scoring
- `CrossEncoderReranker` — ONNX model reranking (feature-gated)
- `DetectedTool` — Tool invocation metadata from stack analysis

---

## VI. Module Hierarchy (Import Tree)

```
lib.rs
├── app_paths
├── benchmarking
├── config_writer (private)
├── daemon (feature-gated)
├── memory_core
│   ├── domain (private → pub use *)
│   │   ├── EventType
│   │   ├── MemoryInput
│   │   ├── SearchResult
│   │   └── ... (11 other types)
│   ├── traits (private → pub use *)
│   │   ├── Storage
│   │   ├── Searcher
│   │   └── ... (26 other traits)
│   ├── embedder (public)
│   │   ├── Embedder (trait)
│   │   └── OnnxEmbedder / PlaceholderEmbedder
│   ├── reranker (public)
│   │   └── CrossEncoderReranker
│   ├── scoring (public)
│   │   └── ScoringParams, decay/weight functions
│   └── storage (public)
│       └── sqlite (public)
│           ├── mod.rs (SqliteStorage + 19 impls)
│           ├── schema.rs
│           ├── entities.rs
│           ├── crud.rs
│           ├── session.rs
│           ├── search.rs
│           ├── nlp.rs
│           ├── temporal.rs
│           ├── graph.rs
│           ├── hot_cache.rs
│           ├── lifecycle.rs
│           ├── helpers.rs
│           ├── query_classifier.rs
│           ├── admin.rs
│           ├── advanced.rs
│           ├── conn_pool.rs
│           ├── embedding_codec.rs
│           └── tests.rs
├── setup
├── tool_detection (private)
└── uninstall

main.rs (CLI + MCP dispatcher)
  └── routes to: setup, config_writer, tool_detection, daemon, mcp_server

mcp_server.rs (standalone MCP protocol handler)
  └── 19 tools mapping to memory_core traits
```

---

## VII. Concerns Analysis: Files Doing Multiple Things

### Critical Multi-Concern Files

| File | Concern Count | Concerns | Severity |
|------|---|----------|----------|
| `sqlite/mod.rs` | 19+ | CRUD, search, graph, relationships, versions, feedback, expiration, profiles, checkpoints, reminders, lessons, maintenance, welcome, backups, stats | **CRITICAL** |
| `sqlite/admin.rs` | 4 | Health checks, backup/restore, consolidation, rotation | HIGH |
| `sqlite/advanced.rs` | 4 | Compaction, graph traversal, feedback, deduplication | HIGH |
| `mcp_server.rs` | 4 | Protocol marshaling, 19 tools (store, retrieve, search, update, delete, graph, etc.), validation, error mapping | HIGH |
| `main.rs` | 3 | Tokio runtime, daemon HTTP, MCP stdio, CLI fallback | HIGH |
| `sqlite/helpers.rs` | 3 | Similarity metrics, text analysis, keyword extraction | MEDIUM |
| `cli.rs` | 3 | Argument parsing, command dispatch, output formatting | MEDIUM |
| `tool_detection.rs` | 3 | Stack parsing, regex detection, classification | MEDIUM |
| `setup.rs` | 2 | Interactive prompts, config generation | MEDIUM |
| `sqlite/session.rs` | 2 | Session lifecycle, summary generation | LOW (scoped) |
| `sqlite/crud.rs` | 2 | Insert/update/delete + auto-relationship logic | MEDIUM |

### Refactoring Recommendations Priority

**Phase 1 (Breaking)**: Extract `sqlite/mod.rs` trait dispatch logic
**Phase 2 (High Value)**: Decompose `mcp_server.rs` tools into submodules
**Phase 3 (Polish)**: Split `admin.rs` and `advanced.rs` by concern
**Phase 4 (Nice to Have)**: Consolidate `setup.rs` + `config_writer.rs`

---

## VIII. Test Coverage

- **Primary**: `sqlite/tests.rs` (7,632 lines) — Integration tests for all SQLite operations
- **In-module**: Tests in `memory_core/mod.rs` (87 lines) for pipeline mocks
- **Helpers**: `test_helpers.rs` (75 lines) for mock implementations

---

## IX. Key Architectural Insights

1. **Clear Separation**: Domain (traits) → Storage (impl) → Server (dispatch)
2. **Single Storage**: SQLite monolith with 19 trait implementations — should be delegated
3. **Pluggable Embeddings**: Placeholder (testing) and ONNX (production) — good design
4. **Scoring Decoupling**: Scoring logic in separate module, reused by search and compaction
5. **Cache Layering**: Query-result cache (LRU) + hot-tier memory cache for performance
6. **MCP Protocol**: Unified interface for AI agents via stdio transport

---

## X. Statistics Summary

- **Total Rust Files**: 41
- **Total Lines of Code**: 49,871
- **God Modules (>500 lines)**: 9
- **Public Traits**: 28
- **Core Domain Structs**: 11
- **Module Nesting Depth**: 3 (lib → memory_core → storage → sqlite)
- **Largest File**: `sqlite/tests.rs` (7,632 lines, mostly test cases)
- **Largest Implementation**: `mcp_server.rs` (2,709 lines, protocol + tools)
- **Storage Subsystem Ratio**: 37% of codebase (18,251 / 49,871 lines)

---

## XI. Closure: Stronghold Findings

| Metric | Value | Status |
|--------|-------|--------|
| Files Scanned | 41 | ✓ Complete |
| Lines Analyzed | 49,871 | ✓ Complete |
| God Modules Found | 9 | ⚠ Refactoring Needed |
| Traits Catalogued | 28 | ✓ Well-Organized |
| Module Hierarchy | 3 levels | ✓ Clean |
| Concerns Extraction Candidates | 11 files | ⚠ In Progress |
| Critical Hotspot | `sqlite/mod.rs` (19 impls in 1,281 lines) | 🔴 HIGH PRIORITY |

