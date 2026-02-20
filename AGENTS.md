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
| MCP tool behavior | `src/mcp_server.rs` | 12 tools: store/retrieve/delete/update/search/semantic/tag/list/recent/relations/add_relation/health |
| Storage schema/ops | `src/memory_core/storage/sqlite.rs` | Uses `spawn_blocking` for DB I/O |
| Pipeline trait boundaries | `src/memory_core/mod.rs` | 12 traits: Ingestor/Processor/Storage/Retriever/Searcher/Recents/SemanticSearcher/Deleter/Updater/Tagger/Lister/RelationshipQuerier |
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
- Next major parity block after MCP is semantic search + embeddings + vector query path.
- New CLI operations (delete, update, list, relations) use `mcp_storage` directly, not Pipeline.
- Tags stored as JSON arrays in the `tags` TEXT column; queried via SQLite `json_each()`.
