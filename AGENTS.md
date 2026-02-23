# PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-20
**Branch:** main

## OVERVIEW

romega-memory is a Rust rewrite of omega-memory focused on parity, local portability, and MCP-first usage.
Current implementation is SQLite-backed memory storage with CLI + MCP stdio server and smoke-tested integration.

## STRUCTURE

```text
romega-memory/
├── src/                  # Runtime code: CLI, MCP server, memory core
├── tests/                # Integration tests (MCP child-process smoke)
├── conductor/            # Product/tracks/style/workflow source of truth
├── .github/workflows/    # CI checks (fmt, clippy, tests)
├── .mcp.json             # Project MCP launcher config
└── Cargo.toml            # Dependencies and feature surface
```

## WHERE TO LOOK

| Task | Location | Notes |
|---|---|---|
| CLI command wiring | `src/cli.rs`, `src/main.rs` | Add enum variant + match arm together |
| MCP tool behavior | `src/mcp_server.rs` | 30 tools: store/retrieve/delete/update/search/semantic/tag/list/recent/relations/add_relation/health/stats/export/import/advanced_search/similar/traverse/phrase_search/feedback/sweep/checkpoint/resume_task/profile/remind/lessons/maintain/welcome/protocol/stats_extended |
| Storage schema/ops | `src/memory_core/storage/sqlite.rs` | Uses `spawn_blocking` for DB I/O; FTS5 virtual table for full-text search |
| Pipeline trait boundaries | `src/memory_core/mod.rs` | 26 traits: Ingestor/Processor/Storage/Retriever/Searcher/Recents/SemanticSearcher/Deleter/Updater/Tagger/Lister/RelationshipQuerier/Embedder/AdvancedSearcher/GraphTraverser/SimilarFinder/PhraseSearcher/FeedbackRecorder/ExpirationSweeper/ProfileManager/CheckpointManager/ReminderManager/LessonQuerier/MaintenanceManager/WelcomeProvider/StatsProvider |
| Scoring system | `src/memory_core/scoring.rs` | Type weights, priority factors, time decay, word overlap, Jaccard similarity |
| Embedding system | `src/memory_core/embedder.rs` | `Embedder` trait, `PlaceholderEmbedder`, `OnnxEmbedder` (feature-gated), model download |
| FTS5 search | `src/memory_core/storage/sqlite.rs` | Standalone FTS5 table synced on store/update/delete; LIKE fallback |
| Integration protocol checks | `tests/mcp_smoke.rs` | Hermetic HOME/USERPROFILE isolation |
| Product direction/tracks | `conductor/product.md`, `conductor/tracks.md` | Parity target and sequencing |

## CONVENTIONS

- Run strict local gate before pushing: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`.
- Keep runtime logs on stderr in MCP mode; stdout must stay protocol-clean.
- Keep blocking SQLite work inside `tokio::task::spawn_blocking`.
- Preserve additive command/tool behavior; do not break `ingest`, `process`, `retrieve`, `serve`.
- Follow semantic commits: `<type>(<scope>): <description>`.

## ANTI-PATTERNS (THIS PROJECT)

- Do not use `unwrap()`/`expect()` in production paths.
- Do not block async executor with direct sync DB I/O.
- Do not merge with unresolved review comments; close all bot/human threads.
- Do not leave CI-parity checks unrun locally even if remote CI is infra-blocked.
- Do not mix MCP protocol output with app logs on stdout.

## UNIQUE STYLES

- Conductor workflow is first-class: plans/specs/tracks are maintained alongside code.
- Trait-first architecture in `memory_core` allows incremental backend and processor replacement.
- MCP development is expected to be testable locally with `.mcp.json` and smoke tests.

## COMMANDS

```bash
# local parity gate
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features

# run app modes
cargo run -- ingest "hello"
cargo run -- retrieve "<memory-id>"
cargo run -- serve

# review loop helpers
gh pr view <num> --json reviews,comments,statusCheckRollup
gh api repos/George-RD/romega-memory/pulls/<num>/comments
```

## NOTES

- CI currently has external billing instability; local strict verification remains mandatory.
- Keep MCP smoke tests hermetic (temp HOME/USERPROFILE) to avoid mutating user state.
- Real embeddings implemented via `Embedder` trait with `OnnxEmbedder` (bge-small-en-v1.5, 384-dim) and `PlaceholderEmbedder` (SHA256 fallback, 32-dim).
- Feature flag `real-embeddings` (default ON) controls ONNX dependency inclusion.
- Model auto-downloads from HuggingFace to `~/.romega-memory/models/bge-small-en-v1.5/` on first use.
- `download-model` CLI command for explicit model pre-download.
- New CLI operations (delete, update, list, relations, stats, export, import, feedback, sweep, checkpoint, resume-task, profile, remind, lessons, maintain, welcome, protocol, stats-extended) use `mcp_storage` directly, not Pipeline.
- Tags stored as JSON arrays in the `tags` TEXT column; queried via SQLite `json_each()`.
- FTS5 full-text search with BM25 ranking; LIKE fallback for edge cases.
- Memories have importance (0.0–1.0), metadata (JSON), access_count, session_id, event_type, project, priority, entity_id, agent_type, ttl_seconds, canonical_hash fields.
- Relationships have weight (0.0–1.0), metadata (JSON), and created_at fields.
- Cross-session profile uses `user_profile` table with key/value JSON payload rows.
- Export/import supports full JSON data portability including relationships and all extended fields.
- Traits use struct-based signatures: `MemoryInput` (store), `MemoryUpdate` (update), `SearchOptions` (search/recent/semantic/tag/list).
- Event types are validated against `VALID_EVENT_TYPES`; priority auto-maps from event type via `default_priority_for_event_type`.
- Schema migrations are additive (ALTER TABLE ADD COLUMN with error ignoring for existing DBs).
- Store lifecycle includes canonicalized-content dedup, event-type Jaccard dedup thresholds, and non-fatal auto-relate relationship linking.
- TTL lifecycle includes auto-assignment by event type, explicit overrides, feedback scoring signals, and expiration sweeping cleanup.
- Advanced search uses multi-phase scoring: vector similarity + FTS5 BM25 + type weighting + time decay + word overlap + importance boost + dedup.
- Graph traversal via BFS with configurable max_hops (1-5), min_weight, and edge type filtering.
- Scoring module (`scoring.rs`) holds parity constants: TYPE_WEIGHTS, DEFAULT_PRIORITY, and scoring helper functions.
- `SearchOptions` extended with `importance_min`, `created_after`, `created_before`, `context_tags` for advanced filtering.
