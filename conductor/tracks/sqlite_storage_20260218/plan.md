# Implementation Plan: SQLite Storage Backend

## Phase 1: Database Infrastructure & Initialization
- [x] Task: Add `rusqlite` and `uuid` to `Cargo.toml`.
- [x] Task: Create `src/memory_core/storage/mod.rs` and `src/memory_core/storage/sqlite.rs` modules.
- [x] Task: Implement `SqliteStorage` struct and its initialization logic.
    - [x] Write unit tests for directory and database file creation at `~/.romega-memory/`.
    - [x] Implement `SqliteStorage::new()` with auto-initialization logic.
- [x] Task: Define the SQLite schema for `memories` and `relationships` tables.
    - [x] Write unit tests to verify table and column existence after initialization.
    - [x] Implement schema migration/initialization SQL.
- [ ] Task: Conductor - User Manual Verification 'Phase 1: Database Infrastructure & Initialization' (Protocol in workflow.md)

## Phase 2: Storage & Retrieval Implementation (TDD)
- [x] Task: Implement the `Storage` trait for `SqliteStorage`.
    - [x] Write failing tests for `store()` (verify data and metadata persistence).
    - [x] Implement `store()` to pass tests (including SHA-256 hashing).
- [x] Task: Implement the `Retriever` trait for `SqliteStorage`.
    - [x] Write failing tests for `retrieve()` (verify content and `last_accessed_at` update).
    - [x] Implement `retrieve()` to pass tests.
- [x] Task: Implement basic relationship storage.
    - [x] Write failing tests for linking two memories.
    - [x] Implement a method (e.g., `add_relationship`) to pass tests.
- [ ] Task: Conductor - User Manual Verification 'Phase 2: Storage & Retrieval Implementation' (Protocol in workflow.md)

## Phase 3: Pipeline Integration & CLI Update
- [x] Task: Update `src/memory_core/mod.rs` to export the new SQLite storage module.
- [x] Task: Integrate `SqliteStorage` into the main `Pipeline` in `src/main.rs`.
    - [x] Replace `PlaceholderPipeline` with `SqliteStorage` for storage and retrieval components.
- [x] Task: Implement the "Default vs. Advanced" configuration flow in the CLI.
    - [x] Update `src/cli.rs` with necessary flags or a configuration command.
- [ ] Task: Conductor - User Manual Verification 'Phase 3: Pipeline Integration & CLI Update' (Protocol in workflow.md)

## Phase 4: Full CRUD + Tags + Relations Query
- [x] Task: Add `Deleter` trait and `SqliteStorage::delete()` implementation with cascade.
- [x] Task: Add `Updater` trait and `SqliteStorage::update()` with content_hash/embedding recalculation.
- [x] Task: Add tags support — `Storage::store()` accepts tags (JSON array), `Tagger` trait with `json_each()` query.
- [x] Task: Add `Lister` trait and paginated list with total count.
- [x] Task: Add `RelationshipQuerier` trait for bi-directional relationship queries.
- [x] Task: Wire all 5 features to CLI commands (delete, update, list, relations, --tags on ingest/process).
- [x] Task: Wire all features as MCP tools (memory_delete, memory_update, memory_tag_search, memory_list, memory_relations, memory_add_relation).
- [x] Task: Unit tests for all new SQLite operations (15 tests).
- [x] Task: Integration smoke tests for MCP and CLI roundtrips.
- [x] Task: Strict gate passes (fmt, clippy -D warnings, test --all-features).
- [ ] Task: Conductor - User Manual Verification 'Phase 4: Full CRUD + Tags + Relations Query' (Protocol in workflow.md)
