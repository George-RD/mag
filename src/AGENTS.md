# SOURCE MAP

## OVERVIEW
`src/` contains executable runtime paths: CLI dispatch, MCP server, and trait-driven memory core.

## STRUCTURE
```text
src/
├── main.rs               # Process entrypoint and command dispatch
├── cli.rs                # Clap command surface
├── mcp_server.rs         # MCP stdio server + tool handlers
└── memory_core/
    ├── mod.rs            # Core traits + Pipeline orchestration
    ├── embedder.rs       # Embedder trait + PlaceholderEmbedder + OnnxEmbedder
    ├── scoring.rs        # Search scoring: type weights, priority factors, time decay, word overlap, Jaccard
    └── storage/
        ├── mod.rs        # Storage exports
        └── sqlite.rs     # SQLite implementation
```

## WHERE TO LOOK
| Task | File | Notes |
|---|---|---|
| Add CLI command | `src/cli.rs` + `src/main.rs` | Keep parser and runtime match in sync |
| Add MCP tool | `src/mcp_server.rs` | Register via `#[tool]` in `#[tool_router]` block |
| Change storage behavior | `src/memory_core/storage/sqlite.rs` | Ensure async-safe DB access |
| Introduce new core stage | `src/memory_core/mod.rs` | Implement trait, then wire Pipeline |
| Embedding behavior | `src/memory_core/embedder.rs` | `Embedder` trait, `OnnxEmbedder` (feature-gated), `PlaceholderEmbedder` |
| New operations via SqliteStorage | `src/main.rs` | New CLI ops use `mcp_storage` directly (not Pipeline) |

## FEATURE SURFACE

### CLI Commands (19)

`ingest`, `process`, `retrieve`, `delete`, `update`, `list`, `relations`, `search`, `semantic-search`, `recent`, `stats`, `export`, `import`, `download-model`, `advanced-search`, `similar`, `traverse`, `phrase-search`, `serve`

### MCP Tools (19)

`memory_store`, `memory_retrieve`, `memory_delete`, `memory_update`, `memory_search`, `memory_semantic_search`, `memory_tag_search`, `memory_list`, `memory_recent`, `memory_relations`, `memory_add_relation`, `memory_health`, `memory_stats`, `memory_export`, `memory_import`, `memory_advanced_search`, `memory_similar`, `memory_traverse`, `memory_phrase_search`

### Core Traits (17)

`Ingestor`, `Processor`, `Storage`, `Retriever`, `Searcher`, `Recents`, `SemanticSearcher`, `Deleter`, `Updater`, `Tagger`, `Lister`, `RelationshipQuerier`, `Embedder`, `AdvancedSearcher`, `GraphTraverser`, `SimilarFinder`, `PhraseSearcher`

### Domain Structs

`MemoryInput` (store params), `MemoryUpdate` (update params), `SearchOptions` (filter by event_type/project/session_id/importance_min/created_after/created_before/context_tags), `SearchResult`, `SemanticResult`, `Relationship`, `ListResult`, `GraphNode`

## CONVENTIONS
- `main.rs` initializes tracing to stderr; preserve this in server mode.
- MCP tools must return stable payloads (`CallToolResult`) and clear error mapping.
- Runtime logs should use metadata (ids, content lengths), not raw content dumps.
- For DB operations called from async contexts, wrap sync SQLite work with `spawn_blocking`.

## ANTI-PATTERNS
- Do not perform direct `rusqlite` calls on async runtime threads.
- Do not change command/tool names without migration rationale; integrations depend on them.
- Do not add stdout logging in server path; protocol corruption risk.
