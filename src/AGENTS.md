# SOURCE MAP

## OVERVIEW
`src/` contains executable runtime paths: CLI dispatch, MCP server, and trait-driven memory core.

## STRUCTURE
```text
src/
├── main.rs               # Process entrypoint and command dispatch
├── cli.rs                # Clap command surface
├── mcp/                  # MCP stdio server + tool handlers
│   ├── mod.rs            # Server struct, #[tool_router] delegation, TOOL_REGISTRY
│   ├── request_types.rs  # All request/response structs
│   ├── validation.rs     # Validation constants + require_finite
│   └── tools/            # Tool handler implementations by concern
│       ├── storage.rs    # store/retrieve/delete + memory facade
│       ├── search.rs     # search, list
│       ├── relations.rs  # relations (list/add/traverse/version_chain)
│       ├── lifecycle.rs  # update, feedback, lifecycle
│       ├── session.rs    # checkpoint/remind/lessons/profile/session_info
│       └── facades.rs    # memory_manage, memory_session, memory_admin
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
| Add MCP tool | `src/mcp/mod.rs` + `src/mcp/tools/*.rs` | Add handler in `tools/`, register via `#[tool]` in `mod.rs` `#[tool_router]` block |
| Change storage behavior | `src/memory_core/storage/sqlite.rs` | Ensure async-safe DB access |
| Introduce new core stage | `src/memory_core/mod.rs` | Implement trait, then wire Pipeline |
| Embedding behavior | `src/memory_core/embedder.rs` | `Embedder` trait, `OnnxEmbedder` (feature-gated), `PlaceholderEmbedder` |
| New operations via SqliteStorage | `src/main.rs` | New CLI ops use `mcp_storage` directly (not Pipeline) |

## FEATURE SURFACE

### CLI Commands (30)

`ingest`, `process`, `retrieve`, `delete`, `update`, `list`, `relations`, `search`, `semantic-search`, `recent`, `stats`, `export`, `import`, `feedback`, `sweep`, `checkpoint`, `resume-task`, `profile`, `remind`, `lessons`, `download-model`, `advanced-search`, `similar`, `traverse`, `phrase-search`, `maintain`, `welcome`, `protocol`, `stats-extended`, `serve`

### MCP Tools (19)

`memory_store`, `memory_store_batch`, `memory_retrieve`, `memory_delete`, `memory_update`, `memory_search`, `memory_list`, `memory_relations`, `memory_feedback`, `memory_lifecycle`, `memory_checkpoint`, `memory_remind`, `memory_lessons`, `memory_profile`, `memory_admin`, `memory_session_info`, `memory`, `memory_manage`, `memory_session`

**Migration note (from pre-consolidation tool set):**
Four tools were merged into the two consolidated tools above:
- `memory_health` → `memory_admin` with `action=health` (+ optional `detail=basic|stats|types|sessions|digest|access_rate`)
- `memory_export` → `memory_admin` with `action=export` or `action=import`
- `memory_welcome` → `memory_session_info` with `mode=welcome`
- `memory_protocol` → `memory_session_info` with `mode=protocol`

All functionality is preserved; only the tool entry-points changed. Update any hardcoded tool calls in prompts or integrations to use the new names above.

### Core Traits (26)

`Ingestor`, `Processor`, `Storage`, `Retriever`, `Searcher`, `Recents`, `SemanticSearcher`, `Deleter`, `Updater`, `Tagger`, `Lister`, `RelationshipQuerier`, `Embedder`, `AdvancedSearcher`, `GraphTraverser`, `SimilarFinder`, `PhraseSearcher`, `FeedbackRecorder`, `ExpirationSweeper`, `ProfileManager`, `CheckpointManager`, `ReminderManager`, `LessonQuerier`, `MaintenanceManager`, `WelcomeProvider`, `StatsProvider`

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
