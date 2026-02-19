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
