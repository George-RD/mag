# MAG Reliability & Observability Foundation

**Date:** 2026-05-04  
**Status:** Design approved  
**Approach:** Fix-First, Then Harness (Approach A)

## Problem Statement

MAG cannot be relied upon as a daily-use memory system. Hook and plugin bugs prevent reliable data capture, scoring semantics silently discard high-quality candidates, and critical installation paths are untested. The system lacks observability — users cannot see what it ingests, how it ranks, or why it suggests context. Without trust, advanced features (graph enrichment, passive sidecar, connector architecture) are built on quicksand.

## Goals

1. **Fix the foundation** — eliminate known bugs in hooks, scoring, and setup/doctor paths.
2. **Make it observable** — build instrumentation and UIs so users can see, tune, and trust MAG behavior.
3. **Validate agentically** — create a harness that connects MAG to agentic benchmarks (AMA-Bench, MemoryArena) and real coding systems, measuring not just retrieval recall but task completion and context efficiency.
4. **Abstract integration** — a single `AgentMemoryHarness` adapter serves benchmarks and coding systems alike, minimizing core code changes.

## Non-Goals

- New retrieval algorithms or embedding models (out of scope; benchmark improvements come after trust).
- Connector/translator architecture overhaul (deferred until foundation is solid).
- Wikipedia-scale benchmarking (backlogged issue #10).

---

## Phase 1: Bug Sweep (Foundation Repair)

### 1.1 Hook / Plugin System Fixes

**Issues:** #255, #257, #259, #243

**#255 — `hooks.json` missing `hooks` wrapper breaks plugin loading**
- **Fix:** Wrap `plugin/hooks/hooks.json` in `{"hooks": {...}}`.
- **Verification:** Load plugin in test Claude Code project; confirm hooks register.

**#257 — Hook scripts don't read stdin JSON, `session_id` always unknown**
- **Fix:** Rewrite `session-start.sh` and `session-end.sh` to read stdin via `cat | jq` (match `pre-compact.sh` pattern).
- **Verification:** Integration test with JSON payload on stdin.

**#259 — Uninstall doesn't clean up `auto-capture.log`**
- **Fix:** Add `auto-capture.log` and `auto-capture.jsonl` removal to `src/uninstall.rs`.
- **Verification:** Unit test with `with_temp_home`.

**#243 — Session-start hook silently succeeds when models are missing**
- **Fix:** Emit warning message instead of empty OK when models absent.
- **Verification:** Test with missing model dir; assert warning emitted.

### 1.2 Scoring / Search Fix

**Issue:** #323 — keyword-only search truncates FTS candidates at `limit` before rescoring.
- **Fix:** Pass `ctx.candidate_limit` (oversampled, 20×, clamped [100, 5000]) instead of `ctx.limit`.
- **Risk:** May change LoCoMo scores measurably.
- **Verification:** `./scripts/bench.sh --gate` before/after. Unit test for low-BM25/high-overlap candidate survival.

### 1.3 Test Coverage Gaps

**Issue:** #245 — Missing coverage for setup model download, doctor model path, cross-encoder check.
- **Fix:** Use `with_temp_home` to unit-test `model_dir()` and `cross_encoder_model_dir()`. Extract doctor path logic into pure testable functions.
- **Verification:** All four gaps have dedicated unit tests.

### Phase 1 Exit Criteria

- [ ] All 6 issues closed with passing tests.
- [ ] `./scripts/bench.sh --gate` shows no regression (>2pp investigated, >5pp blocked).
- [ ] `cargo test --all-features` and clippy pass.
- [ ] Manual smoke test: fresh install → setup → doctor → hook fires → memory captured.

---

## Phase 2: Agent Memory Harness + Benchmark Integration + Observability

### 2.1 AgentMemoryHarness — The Adapter Layer

A single Rust trait that serves benchmarks, coding systems, and future integrations:

```rust
pub trait AgentMemoryHarness {
    fn observe(&self, turn: AgentTurn) -> Result<()>;
    fn retrieve_context(&self, query: &str) -> Result<String>;
    fn inject(&self, context: &str, agent_prompt: &mut Prompt) -> Result<()>;
    
    // Observability hooks
    fn log_construction(&self, event: ConstructionEvent);
    fn log_retrieval(&self, event: RetrievalEvent);
    fn log_injection(&self, event: InjectionEvent);
    fn log_failure(&self, event: FailureEvent);
}
```

**Design principle:** The harness sits between external systems and MAG core. External systems call the harness; the harness calls MAG's existing MCP/store/search traits. MAG core requires zero changes to support new integrations.

**Why this isn't over-engineering:** The harness is thin. It doesn't add new memory logic — it adds **boundary stability**. Every new integration (benchmark, IDE, coding agent) implements the same interface, so we learn integration patterns once and reuse them.

### 2.2 Benchmark Adapters

| Adapter | Benchmark | Integration Pattern | What We Learn |
|---------|-----------|---------------------|---------------|
| `AmaBenchDriver` | AMA-Bench (Feb 2026) | Replay completed trajectories via `observe()`, Q&A via `retrieve_context()` | Does MAG retain coding context? Baseline quality signal. |
| `MemoryArenaDriver` | MemoryArena (Feb 2026) | Live agent loop: `observe()` after every action, `retrieve_context()` before next action | Does MAG help agents make better decisions? The real test. |
| `LocomoDriver` | LoCoMo (existing) | Refactored into harness | Regression gate. |
| `LongMemEvalDriver` | LongMemEval (existing) | Refactored into harness | Regression gate. |

**AMA-Bench specifics:**
- Real + synthetic trajectories across 6 domains (Web, SWE-bench, Text2SQL, Gaming, etc.)
- Software Engineering domain is literal SWE-bench trajectories (Claude + OpenHands)
- Two-stage Python interface: `memory_construction(traj_text)` → `memory_retrieve(memory, question)`
- Python adapter wraps MAG's `mag serve` MCP interface
- Scores: AMA-Agent 57.2%, HippoRAG2 44.8%, MemoRAG 46.1%, MemGPT 33.0% (hard, discriminating)

**MemoryArena specifics:**
- 4 environments: Bundled Web Shopping, Group Travel Planning, Progressive Web Search, Sequential Formal Reasoning
- Multi-session agent-environment loop where later tasks depend on earlier learning
- Tests Memory-Agent-Environment interaction, not just recall
- Key finding: agents with near-perfect LoCoMo scores drop to 0–12% task success

**Bias mitigation:**
- AMA-Bench created by same team as AMA-Agent — potential bias toward causality graphs
- Mitigation: use as one signal among many; run independent metrics (context efficiency, token savings); compare against multiple baselines on same harness

### 2.3 Observability — Instrumentation First, UI Second

**Deterministic metrics (always-on, cheap):**
- Retrieval recall@k per query type
- Token count: retrieved context vs. full history
- Latency: construction, retrieval, total inference
- Memory store growth rate, compaction ratio
- Failure mode tags: "no candidates", "candidates found but wrong", "correct candidate ranked too low", "belief drift"

**Semantic metrics (periodic, LLM-judged):**
- Context relevance: does retrieved context help answer the question?
- Task success rate with/without memory
- Belief drift detection across sessions

**Structured trace output:**
Every harness run produces a JSON trace with turn-by-turn logs of what was stored, retrieved, injected, and why it succeeded or failed. This is the raw material for both automated analysis and TUI/WebUI display.

### 2.4 LLM Abstraction

**LiteLLM proxy** running locally on `localhost:4000`.
- MAG's existing `reqwest` client points at OpenAI-compatible endpoint
- `litellm_config.yaml` switches between OpenAI/Anthropic/Gemini/local models
- No provider-specific code in MAG benchmark layer
- Enables easy A/B testing: "does MAG work better with GPT-5.2 or Claude 4.5 as the agent?"

### Phase 2 Exit Criteria

- [ ] `AgentMemoryHarness` trait defined and implemented for MAG.
- [ ] AMA-Bench adapter runs Software Engineering domain; produces scores + failure-mode traces.
- [ ] MemoryArena adapter runs at least one environment (e.g., Progressive Web Search).
- [ ] Comparative run: MAG vs. at least one other memory system (MemGPT or MemoryBank) on same scenario.
- [ ] Observability traces explain *why* each failure occurred.
- [ ] Token efficiency metric measured: task accuracy per token used.

---

## Phase 3: Passive Ingestion + TUI/WebUI

Now the observability layer displays a system with validated benchmark behavior.

### 3.1 Shared Observability Backend

Event bus consumed by both TUI and WebUI:
- `IngestEvent`, `MemoryCommitted`, `RelationFormed`, `SuggestionGenerated`, `EvidenceUpdated`, `SystemHealth`
- Bounded in-memory ring buffer (last 10,000) + optional `~/.mag/events.jsonl`

### 3.2 Passive Log Ingestion Engine

Watches log files instead of requiring hooks:
- Sources: Claude Code `~/.claude/...`, generic globs
- Processing: parse → chunk → enrich → store → emit
- Standalone module (`src/ingest/`) depending only on `memory_core` traits

### 3.3 TUI — Operational Dashboard

`ratatui` in cmux/tmux pane:
- Ingest stream, session summary, suggestion queue, health bar
- Start in <500ms, <50MB RAM

### 3.4 WebUI — Graph Explorer

Axum server on port 7734:
- Interactive graph (D3/Cytoscape), memory inspector, evidence ledger
- Tuning panel: live scoring weight adjustment with preview
- Bind to `127.0.0.1` only

### Phase 3 Exit Criteria

- [ ] Log ingestion parses Claude Code logs without hooks.
- [ ] TUI displays live stream and allows dismissing suggestions.
- [ ] WebUI serves graph and inspector.
- [ ] User dogfooded for 5+ days, filed 3+ data-quality observations.

---

## Phase 4: Automated E2E Harness & Regression Suite

### 4.1 Dev Plugin Isolation
- `MAG_DATA_DIR=/tmp/mag-e2e-$(uuid)` — fully isolated
- `MAG_CONFIG_DIR=/tmp/mag-e2e-.../config`
- `mag e2e teardown` cleanup

### 4.2 Scripted Workflows
YAML-defined scenarios with assertions on memory state, graph structure, and temporal resolution.

### 4.3 Regression Tracking
- Golden log files + baseline snapshots
- CI GitHub Action: run harness, compare baseline, post delta summary
- Warn at >5% delta, fail at >10%

---

## Architecture

```
External Systems:
  [Claude Code] [Codex] [Cursor] [Benchmark: AMA-Bench] [Benchmark: MemoryArena]
         |           |        |              |                    |
         └───────────┴────────┴──────────────┴────────────────────┘
                                 |
                    ┌────────────┴────────────┐
                    │   AgentMemoryHarness    │  <-- Thin adapter layer
                    │  (observe | retrieve    │
                    │   | inject | log)       │
                    └────────────┬────────────┘
                                 |
              ┌──────────────────┼──────────────────┐
              │                  │                  │
         ┌────┴────┐       ┌────┴────┐       ┌─────┴──────┐
         │   TUI   │       │  WebUI  │       │  Event Bus │
         │(ratatui)│       │(Axum+SPA│       │  + Traces  │
         └─────────┘       └─────────┘       └────────────┘
                                 |
                    ┌────────────┴────────────┐
                    │       MAG Core          │
                    │  (Storage | Search |     │
                    │   Scoring | Embedder)   │
                    └─────────────────────────┘
```

## Error Handling & Safety

- All new modules: `anyhow::Context` + `?`; no `unwrap()`/`expect()` in production.
- Ingest engine: skip malformed lines with warning, never crash watcher.
- TUI: degrade gracefully if not TTY.
- WebUI: `127.0.0.1` only; sanitize query params.
- E2E harness: always teardown isolated dirs, even on panic.

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| #323 fix changes LoCoMo scores | Medium | High | Bench before/after; revert if >5pp regression |
| TUI/WebUI add binary bloat | Low | Medium | Feature-gate (`observability` flag) |
| Log parsing fragile across CC versions | Medium | Medium | Versioned parsers; fallback to raw text |
| E2E harness flaky | Medium | High | Deterministic data; mock time |
| AMA-Bench bias toward AMA-Agent | Medium | Medium | Use as one signal; compare multiple systems |

## Decision Log

| Date | Decision | Rationale | Alternatives Rejected |
|------|----------|-----------|----------------------|
| 2026-05-04 | Approach A (Fix-First) | Building harness on broken foundation gives false signals | Approach B (Harness-First) — too noisy |
| 2026-05-04 | Build `AgentMemoryHarness` adapter | Single integration pattern serves benchmarks + coding systems | Separate adapters per benchmark — duplication |
| 2026-05-04 | AMA-Bench + MemoryArena both in Phase 2 | AMA-Bench gives coding trajectories; MemoryArena gives agent-loop patterns | AMA-Bench only — misses the "memory helps agent act" test |
| 2026-05-04 | LiteLLM proxy for LLM abstraction | Zero provider code in MAG; one config switches models | Rust trait per provider — more maintenance |
| 2026-05-04 | Instrumentation before UI | TUI shows failure modes; harness generates them | UI-first — pretty but uninformative |
| 2026-05-04 | Harness supports other memory systems | Comparative observability reveals WHY systems differ | MAG-only — no external reference |

## Open Questions

1. Should TUI/WebUI be behind single `observability` flag or separate `tui`/`webui` flags?
2. Exact Claude Code log schema? (Need to inspect `~/.claude/` on live system.)
3. E2E harness: real ONNX embedder or fast placeholder for CI speed?
4. Fact vs. belief separation: simplest evidence-strengthening model to prototype?
5. Should harness memory injection be automatic or agent-requested?

## Success Metrics

- **Phase 1:** Zero open P0/P1 bugs in hooks, scoring, setup.
- **Phase 2:** MAG runs on AMA-Bench SWE domain + MemoryArena web-search; comparative traces explain failures.
- **Phase 3:** Daily dogfooding for 5+ days without trust-based disabling.
- **Phase 4:** E2E suite catches regression within first month.
