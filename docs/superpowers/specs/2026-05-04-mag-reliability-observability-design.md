# MAG Reliability & Observability Foundation

**Date:** 2026-05-04
**Approach:** Fix-First, Then Harness (Approach A)
**Status:** Design approved

## Problem Statement

MAG cannot be relied upon as a daily-use memory system. Hook and plugin bugs prevent reliable data capture, scoring semantics silently discard high-quality candidates, and critical installation paths are untested. The system lacks observability — users cannot see what it ingests, how it ranks, or why it suggests context. Without trust, advanced features (graph enrichment, passive sidecar, connector architecture) are built on quicksand.

## Goals

1. **Fix the foundation** — eliminate known bugs in hooks, scoring, and setup/doctor paths.
2. **Make it observable** — build passive ingestion with TUI and WebUI so users can see, tune, and trust MAG behavior.
3. **Automate verification** — create an E2E harness that prevents regression and validates retrieval quality.

## Non-Goals

- New retrieval algorithms or embedding models (out of scope; benchmark improvements come after trust).
- Connector/translator architecture overhaul (deferred until foundation is solid).
- AutoMem parity or competitive benchmarking (Phase 3 may include this, but it is not the primary goal).
- Wikipedia-scale benchmarking (backlogged issue #10).

## Phase 1: Bug Sweep (Foundation Repair)

### 1.1 Hook / Plugin System Fixes

**Issues:** #255, #257, #259, #243

**#255 — `hooks.json` missing `hooks` wrapper breaks plugin loading**
- **Root cause:** `plugin/hooks/hooks.json` uses flat format with event names at root. Claude Code expects `{"hooks": {...events...}}`.
- **Fix:** Wrap the existing JSON object in a top-level `hooks` key. Update any documentation that references the flat format.
- **Verification:** Load the plugin in a test Claude Code project and confirm hooks register without error.

**#257 — Hook scripts don't read stdin JSON, `session_id` always unknown**
- **Root cause:** `session-start.sh` and `session-end.sh` use dead environment variables instead of reading stdin JSON (Claude Code passes `session_id`, `last_assistant_message` via stdin).
- **Fix:** Rewrite both scripts to read stdin via `cat | jq` following the pattern already used in `pre-compact.sh`. Extract and export `session_id` and relevant fields.
- **Verification:** Add an integration test that invokes the hook scripts with a JSON payload on stdin and asserts the correct variables are parsed.

**#259 — Uninstall doesn't clean up `auto-capture.log`**
- **Root cause:** `src/uninstall.rs` never removes `~/.mag/auto-capture.log` or future `auto-capture.jsonl`.
- **Fix:** Add explicit removal of `auto-capture.log` and `auto-capture.jsonl` to the uninstall routine. Use the existing `app_paths.rs` helpers to resolve the path.
- **Verification:** Unit test `run_uninstall` with a temporary home directory, assert both files are removed.

**#243 — Session-start hook silently succeeds when models are missing**
- **Root cause:** When embedding models haven't been downloaded, the session-start hook returns `OK` with empty content. The user gets zero feedback.
- **Fix:** In `session-start.sh`, after checking for model files, if missing, emit a brief warning message instead of empty OK:
  ```
  ⚠ MAG: embedding models not downloaded — memory recall unavailable.
  Run 'mag download-model' or 'mag setup' to initialize.
  ```
- **Verification:** Test with `with_temp_home` helper where model directory is absent; assert warning is emitted.

### 1.2 Scoring / Search Fix

**Issue:** #323 — `candidate_limit` semantics: keyword-only FTS search truncates before rescoring

- **Root cause:** In `src/memory_core/storage/sqlite/pipeline/scoring.rs`, `run_keyword_only_search` passes `ctx.limit` (the user's final limit) to `fts_search`. Candidates that BM25 ranks outside top-`limit` but that would score highly after overlap/importance/time-decay rescoring are discarded prematurely.
- **Fix:** Pass `ctx.candidate_limit` (oversampled, 20×, clamped [100, 5000]) instead of `ctx.limit`. The abstention gate and final truncate-at-`limit` downstream remain unchanged.
- **Risk:** May change LoCoMo scores measurably.
- **Verification:** Run `./scripts/bench.sh --gate` before and after. Accept if delta is within ±2pp; investigate if >2pp. Add a unit test in `scoring.rs` that constructs a keyword-only search with low BM25 but high word-overlap, asserting the candidate survives oversampling.

### 1.3 Test Coverage Gaps

**Issue:** #245 — Missing coverage for setup model download, doctor model path, cross-encoder check

- **Critical gaps:**
  1. `setup.rs` `run_setup()` model download phase (lines 73-85) — zero coverage, including error paths.
  2. `main.rs` doctor model path fix (lines 1322-1328) — no test verifies correct subdirectory (`bge-small-en-v1.5-int8`) vs old `model_root` direct use.
  3. `main.rs` cross-encoder `Warn` check (lines 1470-1479) — `run_doctor()` has no tests at all.
  4. `model_dir()` (`embedder.rs:694`) and `cross_encoder_model_dir()` (`reranker.rs:265`) — new pub functions, no unit tests.
- **Fix approach:**
  - Use existing `with_temp_home` test helper to unit-test `model_dir()` and `cross_encoder_model_dir()`.
  - Extract doctor path-resolution logic into pure testable functions.
  - For setup download, mock the download functions or test path resolution and error handling directly.
- **Verification:** All four gaps have dedicated unit tests; `cargo test --all-features` passes.

### Phase 1 Exit Criteria

- [ ] All 6 issues closed with passing tests.
- [ ] `./scripts/bench.sh --gate` shows no regression (>2pp investigated, >5pp blocked).
- [ ] `cargo test --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` pass.
- [ ] Manual smoke test: fresh install → `mag setup` → `mag doctor` → session hook fires → memory is captured.

## Phase 2: Passive Ingestion + Observability Layer

### 2.1 Shared Observability Backend

A single internal event bus (channel-based, in-process) that both TUI and WebUI consume:

**Event types:**
- `IngestEvent` — raw log parsed, memory candidate extracted, confidence score
- `MemoryCommitted` — memory stored in SQLite, with ID, content hash, tags
- `RelationFormed` — source memory, target memory, relation type, strength
- `SuggestionGenerated` — query context, suggested memory IDs, relevance scores, reason
- `EvidenceUpdated` — memory ID, supporting/contradicting signals, new strength
- `SystemHealth` — model loaded, last ingest timestamp, memory count, DB size

**Design principles:**
- Events are append-only and timestamped.
- The bus is optional — MAG works exactly the same if no observer is connected.
- Events are stored in a bounded in-memory ring buffer (last 10,000) and optionally written to `~/.mag/events.jsonl` for post-hoc analysis.

### 2.2 Passive Log Ingestion Engine

Instead of requiring hooks (which depend on Claude Code's integration surface), MAG optionally watches log files on disk:

**Sources:**
- Claude Code: `~/.claude/...` conversation logs (JSON or markdown)
- Generic: any JSONL or markdown file matching a glob pattern
- Future: stdin stream, TCP socket, or MCP tool invocation

**Processing pipeline:**
1. **Watch** — `notify` crate watches configured log directories.
2. **Parse** — extract speaker turns, tool use, file edits, code blocks.
3. **Chunk** — split into memory-sized units (configurable, default 512 tokens).
4. **Enrich** — entity extraction, temporal tagging, topic keywords.
5. **Store** — write to MAG SQLite via existing `store` trait.
6. **Emit** — push events to observability bus.

**Configuration:**
```toml
[ingest]
enabled = true
sources = [
  { type = "claude", path = "~/.claude/conversations" },
  { type = "generic", path = "~/logs/ai-chat/*.jsonl", format = "jsonl" }
]
```

**Boundary:** The ingestion engine is a standalone module (`src/ingest/`) that depends only on `memory_core` traits. It does not change the MCP server or hook behavior.

### 2.3 TUI — Operational Dashboard

A lightweight terminal UI running in a cmux/tmux pane, built with `ratatui`:

**Views:**
- **Ingest Stream** — scrolling list of last N ingest events with timestamp, source, and confidence.
- **Session Summary** — memory count, pending suggestions, last successful search, DB size.
- **Suggestion Queue** — what MAG would inject now, with relevance score and one-line preview.
- **Health Bar** — model status, last ingest time, any warnings (e.g., models missing).

**Interactions:**
- `Enter` on a suggestion to copy it to clipboard or write to a file.
- `d` to dismiss a suggestion.
- `r` to force a manual compact/merge.
- `q` to quit (MAG continues running in background).

**Design constraint:** Must start in <500ms and use <50MB RAM. It's a thin client over the event bus.

### 2.4 WebUI — Graph Explorer & Tuning Interface

A lightweight embedded web server serving a single-page application:

**Server:** Axum (or Actix) on a configurable port (default 7734). Serves static JS/WASM and a JSON API over the event bus.

**Graph View (`/graph`):**
- Interactive force-directed graph (D3.js or Cytoscape.js).
- Nodes = memories (colored by type: fact, opinion, ephemeral). Edges = relations (colored by strength).
- Click a node to open Memory Inspector.
- Filter by time range, tag, entity, or confidence threshold.

**Memory Inspector (`/memory/:id`):**
- Full content, tags, entities, temporal anchors.
- **Evidence Ledger** — list of events that strengthened or weakened this memory, with timestamps and source logs.
- **Version Chain** — if memory was updated or superseded, show the chain.
- **Relation Traversal** — BFS graph expansion from this node.

**Tuning Panel (`/tune`):**
- Live sliders for scoring weights (time decay, word overlap, graph neighbor factor, type weights).
- "Preview" button: run a test query with new weights and see result ranking change.
- "Reset to defaults" button.
- Changes are persisted to `~/.mag/tuning.toml` and applied to the running system.

**Comparison Mode (future / optional):**
- If AutoMem data can be exported or bridged, display both memory graphs side-by-side for the same conversation corpus.

**Security:** Bind to `127.0.0.1` only. No auth required (local only).

### 2.5 Fact vs. Belief / Episodic vs. Semantic Memory (Research Issue)

During Phase 2, create a research issue documenting the need for separate memory classes:

- **Facts / Semantic:** "Project X uses Rust." Supersession is appropriate (newer facts replace outdated ones).
- **Beliefs / Heuristics / Episodic:** "Prompting with chain-of-thought improves results for Y." Newer techniques do not necessarily invalidate older ones; evidence accumulates. Relevance requires **strengthening/weakening** mechanics, not TTL or supersession.

Defer implementation to Phase 3+ but capture the design space: evidence ledger per memory, confidence intervals, Bayesian or simple vote-based updating.

### Phase 2 Exit Criteria

- [ ] Log ingestion engine parses Claude Code logs and stores memories without hooks.
- [ ] TUI starts, displays live ingest stream, and allows dismissing suggestions.
- [ ] WebUI serves graph visualization and Memory Inspector for any stored memory.
- [ ] Tuning panel changes affect live search results.
- [ ] User has dogfooded for at least 5 days, filed at least 3 data-quality observations.

## Phase 3: Automated E2E Harness & Regression Suite

### 3.1 Dev Plugin Isolation

- `MAG_DATA_DIR=/tmp/mag-e2e-$(uuid)` — fully isolated SQLite, models, config.
- `MAG_CONFIG_DIR=/tmp/mag-e2e-.../config` — isolated hooks, tuning, ingest sources.
- Dev plugin wrapper: a thin shell script that sets these env vars before invoking the real `mag` binary.
- Cleanup: `mag e2e teardown` removes the isolated directory.

### 3.2 Scripted Workflows

YAML-defined scenarios:

```yaml
scenario: hello_world
description: "Basic store and retrieve"
steps:
  - cmd: "mag store --content 'Rust uses cargo for builds' --tags 'rust,build'"
    assert:
      exit_code: 0
      output_contains: "Stored memory"
  - cmd: "mag search --query 'how do I build Rust'"
    assert:
      exit_code: 0
      results:
        - content_contains: "cargo"
          relevance: "> 0.7"
```

**Scenario types:**
- **Store/Retrieve:** Basic CRUD, tag search, semantic search.
- **Relation Graph:** Store multiple memories, assert relations are formed.
- **Temporal:** Store dated facts, assert temporal queries resolve correctly.
- **Hook Simulation:** Write synthetic log files, assert ingestion engine captures expected memories.
- **Regression:** Replay a golden conversation log, assert the same memories are extracted as in the baseline.

### 3.3 Assertion Library

Beyond simple string matching:
- **Graph assertions:** `memory X links to entity Y via relation Z with strength > 0.8`
- **Semantic assertions:** `top result for query Q contains concept C`
- **Temporal assertions:** `memory M has timestamp within 1s of event E`
- **Diff assertions:** compare full result sets between runs, report added/removed/changed memories.

### 3.4 Regression Tracking

- Golden log files: real conversation snippets where MAG behavior is known-good.
- Baseline snapshots: JSON dumps of memory state after ingesting each golden log.
- CI GitHub Action:
  - Runs harness on PR.
  - Compares against baseline.
  - Posts PR comment with delta summary (memory count, relation count, top-N overlap score).
  - Warn at >5% delta, fail at >10%.

### Phase 3 Exit Criteria

- [ ] At least 10 E2E scenarios pass in CI.
- [ ] Regression suite runs in <5 minutes.
- [ ] PRs blocked if harness fails.
- [ ] Documentation exists for adding new scenarios.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                     MAG Core (existing)                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │ Storage  │  │ Scoring  │  │ Search   │  │ Embedder │    │
│  │ (SQLite) │  │ Pipeline │  │ Pipeline │  │ (ONNX)   │    │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘    │
│       └─────────────┴─────────────┴─────────────┘           │
│                         │                                    │
│              ┌──────────┴──────────┐                        │
│              │    Memory Traits    │                        │
│              └──────────┬──────────┘                        │
└─────────────────────────┼───────────────────────────────────┘
                          │
         ┌────────────────┼────────────────┐
         │                │                │
    ┌────┴────┐     ┌────┴────┐     ┌─────┴──────┐
    │  Hooks  │     │ Ingest  │     │ Event Bus  │
    │ (fixed) │     │ Engine  │     │ (new)      │
    └────┬────┘     └────┬────┘     └─────┬──────┘
         │               │                │
         └───────────────┴────────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
         ┌────┴────┐    ┌────┴────┐   ┌────┴────┐
         │   TUI   │    │  WebUI  │   │  E2E    │
         │ (ratatui)│   │ (Axum+SPA)│  │Harness  │
         └─────────┘    └─────────┘   └─────────┘
```

## Error Handling & Safety

- All new modules use `anyhow::Context` and `?`; no `unwrap()`/`expect()` in production paths.
- Ingest engine: malformed log lines are skipped with a warning event, never crash the watcher.
- TUI: if terminal is not a TTY, degrade to a simple status line or exit gracefully.
- WebUI: bind to `127.0.0.1` only; sanitize all user-provided query parameters.
- E2E harness: always teardown isolated directories, even on assertion failure or panic.

## Testing Strategy

- **Phase 1:** Unit tests for every bug fix. Integration tests for hook loading and doctor paths.
- **Phase 2:**
  - Ingest engine: test with synthetic log files in `/tmp`.
  - TUI: headless terminal tests with `ratatui`'s test backend or snapshot testing.
  - WebUI: HTTP-level tests with `reqwest` against the Axum app.
- **Phase 3:** Full E2E scenarios as integration tests using the dev plugin isolation.

## Performance Constraints

- Ingest engine: process a 1MB log file in <1s on a modern laptop.
- TUI: render loop at 4Hz, <5% CPU at idle.
- WebUI: graph render for 1,000 nodes must be interactive (<100ms initial layout).
- E2E harness: full suite runs in <5 minutes in CI.

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| #323 fix changes LoCoMo scores unpredictably | Medium | High | Bench before/after; revert if >5pp regression |
| TUI/WebUI add significant binary bloat | Low | Medium | Feature-gate (`observability` flag); TUI and WebUI are separate crates/features |
| Log parsing is fragile across Claude Code versions | Medium | Medium | Versioned parsers; fallback to raw text chunking |
| E2E harness becomes flaky due to timing | Medium | High | Deterministic test data; mock time; retry with jitter only where necessary |

## Open Questions

1. Should the TUI and WebUI be behind a single `observability` feature flag, or separate `tui` and `webui` flags?
2. What is the exact schema of Claude Code conversation logs? (Need to inspect `~/.claude/` on a live system.)
3. Should the E2E harness use the real ONNX embedder or a fast placeholder embedder to keep CI times low?
4. For fact vs. belief separation, what is the simplest evidence-strengthening model we can prototype? (Simple vote count? Time-decayed sum?)

## Success Metrics

- **Phase 1:** Zero open P0/P1 bugs in hooks, scoring, or setup paths.
- **Phase 2:** User uses MAG daily for 5+ days without disabling it due to trust issues.
- **Phase 3:** E2E suite catches at least one regression within the first month of operation.
