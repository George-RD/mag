# MEMORY CORE MAP

## OVERVIEW

`memory_core` defines the domain contracts and orchestration for ingest/process/store/retrieve.

## STRUCTURE

```text
src/memory_core/
├── mod.rs               # Traits + Pipeline orchestration
├── embedder.rs          # Embedder trait + PlaceholderEmbedder + OnnxEmbedder
└── storage/
    ├── mod.rs           # Export surface
    └── sqlite.rs        # SQLite-backed storage + schema + tests
```

## WHERE TO LOOK

| Task | File | Notes |
|---|---|---|
| Trait contract changes | `src/memory_core/mod.rs` | Update trait + Pipeline usage + tests together |
| SQLite schema changes | `src/memory_core/storage/sqlite.rs` | Keep migration-safe additive table updates |
| Retrieval semantics | `src/memory_core/storage/sqlite.rs` | `retrieve` updates `last_accessed_at` in one transaction |
| Relationship behavior | `src/memory_core/storage/sqlite.rs` | FK enforcement + cascade behavior are required |
| Event type validation | `src/memory_core/mod.rs` | `VALID_EVENT_TYPES`, `is_valid_event_type()`, `default_priority_for_event_type()` |
| Struct-based signatures | `src/memory_core/mod.rs` | `MemoryInput`, `MemoryUpdate`, `SearchOptions` replace positional params |
| Embedding generation | `src/memory_core/embedder.rs` | `Embedder` trait with `OnnxEmbedder` (384-dim, feature-gated) and `PlaceholderEmbedder` (32-dim SHA256 fallback) |

## CONVENTIONS

- Keep Pipeline methods thin orchestration; heavy logic belongs in concrete implementations.
- Preserve id defaulting (`Uuid::new_v4`) in pipeline paths when caller omits id.
- Schema initialization must enable `PRAGMA foreign_keys = ON` before DDL.
- SQLite tests should prefer `new_in_memory()` and explicit edge-case assertions.

## ANTI-PATTERNS

- Do not drop/rename existing columns in this phase; favor additive schema evolution.
- Do not bypass transactional writes for coupled read/update flows.
- Do not regress hermetic tests by coupling to user HOME paths.

## COMMANDS

```bash
# focused checks for this area
cargo test memory_core:: --all-features
cargo test sqlite:: --all-features

# full parity gate
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
