# MAG Reliability & Observability Foundation

**Date:** 2026-05-04  
**Status:** Design approved  
**Approach:** Benchmark-Driven Quality First

## Problem Statement

MAG cannot be relied upon as a daily-use memory system. Scoring semantics silently discard high-quality candidates (#323), critical paths are untested (#245), and the system lacks validated patterns for *when* to surface memories to agents. Without knowing whether active memory injection helps agents complete tasks, we cannot trust MAG in real workflows.

## Goals

1. **Fix load-bearing issues** — scoring fix and test coverage only; defer ingestion hook fixes (replaced by log watcher in Phase 3).
2. **Validate retrieval quality** — establish baseline on AMA-Bench coding trajectories.
3. **Learn active injection** — use MemoryArena to discover when and how MAG should surface context to agents.
4. **Build deterministically** — each phase ends with a gate; no advancing until the gate passes.

## Non-Goals

- Fixing ingestion hooks (#255, #257, #259, #243) — superseded by Phase 3 log watcher.
- TUI/WebUI in early phases — start with JSON traces.
- Full MemoryArena coverage — one environment only, to learn the loop pattern.
- Comparative benchmarking against other memory systems — defer until MAG's own scores are stable.

---

## Phase 1: Load-Bearing Fixes

### 1.1 Scoring / Search Fix

**Issue:** #323 — keyword-only search truncates FTS candidates at `limit` before rescoring.
- **Fix:** Pass `ctx.candidate_limit` instead of `ctx.limit`.
- **Verification:** `./scripts/bench.sh --gate` before/after. Unit test for low-BM25/high-overlap survival.

### 1.2 Test Coverage Gaps

**Issue:** #245 — Missing coverage for setup model download, doctor model path, cross-encoder check.
- **Fix:** Use `with_temp_home` to unit-test `model_dir()` and `cross_encoder_model_dir()`. Extract doctor path logic into pure testable functions.
- **Verification:** All four gaps have unit tests.

### Phase 1 Exit Gate

- [ ] `./scripts/bench.sh --gate` passes with no regression (>5pp blocks).
- [ ] `cargo test --all-features` and clippy pass.

---

## Phase 2a: Minimal Harness + AMA-Bench Baseline

### 2.1 Harness Trait (Minimal)

```rust
pub trait AgentMemoryHarness {
    fn observe(&self, turn: AgentTurn) -> Result<()>;
    fn retrieve_context(&self, query: &str) -> Result<String>;
    fn trace(&self, event: serde_json::Value) -> Result<()>;
}
```

- Three methods only. No typed event structs. No `inject()` — callers compose retrieved context into prompts themselves.
- Unstructured JSON trace lines written to `~/.mag/traces/<run-id>.jsonl`.

### 2.2 AMA-Bench Adapter

- Python wrapper around `mag serve` MCP interface.
- Implements AMA-Bench's two-stage protocol: `memory_construction(traj_text, task)` → `memory_retrieve(memory, question)`.
- **Scope:** Software Engineering domain only (SWE-bench trajectories, most relevant to coding).

### 2.3 LLM Config (Minimal)

```rust
struct BenchmarkConfig {
    api_url: String,
    api_key: String,
    model: String,
}
```

- Environment variables, no proxy, no extra services.

### Phase 2a Exit Gate

- [ ] AMA-Bench SWE domain runs end-to-end.
- [ ] Scores + JSON traces produced for every question.
- [ ] Top 3 failure modes identified from traces (e.g., `retrieval_empty`, `wrong_candidate_ranked_first`, `high_latency`).

---

## Phase 2b: MemoryArena + Active Injection Design

### 2.4 MemoryArena Adapter (One Environment)

- **Scope:** Progressive Web Search only (simplest agent loop: search → observe → search again).
- Harness intercepts agent loop: after each action, `observe()` stores; before next action, `retrieve_context()` injects into prompt.

### 2.5 Recall Hook Design

Prototype when MAG should inject context:

| Strategy | Mechanism | Tested In |
|----------|-----------|-----------|
| Passive | Agent asks via MCP `memory_search` | AMA-Bench |
| Active-Threshold | Inject when semantic similarity > threshold | MemoryArena |
| Active-Boundary | Inject on tool-use boundaries | MemoryArena |
| Active-Periodic | Inject every N turns | MemoryArena |

**Goal:** Determine if active injection improves task success over passive retrieval.

### Phase 2b Exit Gate

- [ ] MemoryArena Progressive Web Search runs end-to-end.
- [ ] Task success rate with MAG >= task success rate without MAG.
- [ ] Traces explain which recall strategy worked and why.

---

## Phase 2c: Synthesis & Refactor

Between-phase checkpoint. No new features.

- Review AMA-Bench + MemoryArena traces.
- Identify top 3 failure modes across both benchmarks.
- Refactor MAG's scoring/pipeline to address them.
- Run adversarial review on changes.

### Phase 2c Exit Gate

- [ ] LoCoMo score stable or improved.
- [ ] AMA-Bench score improved vs. Phase 2a.
- [ ] Failure modes from Phase 2a are addressed or explicitly deprioritized with rationale.

---

## Phase 3: Passive Ingestion + Observability

### 3.1 Log Ingestion Engine

Watch log files instead of hooks:
- Sources: Claude Code `~/.claude/...`, generic globs.
- Processing: parse → chunk → enrich → store → emit trace.
- Standalone module (`src/ingest/`), depends only on `memory_core` traits.

### 3.2 TUI

`ratatui` in cmux/tmux pane:
- Ingest stream, session summary, suggestion queue, health bar.
- Start in <500ms, <50MB RAM.
- **Justified now:** Phase 2 traces give us failures to display.

### Phase 3 Exit Gate

- [ ] Log ingestion parses Claude Code logs.
- [ ] TUI displays live stream.
- [ ] User dogfooded for 5+ days, filed 3+ data-quality observations.

---

## Phase 4: E2E Regression

Minimal regression suite:
- Shell script runs AMA-Bench twice, diffs JSON output.
- Golden log files added after manual runs stabilize.
- Defer full CI integration until flakiness is understood.

---

## Architecture

```
[Benchmark: AMA-Bench]     [Benchmark: MemoryArena]
         |                           |
         └───────────┬───────────────┘
                     |
          ┌──────────┴──────────┐
          │ AgentMemoryHarness  │  <-- 3-method trait
          │ (observe | retrieve │
          │  | trace)           │
          └──────────┬──────────┘
                     |
          ┌──────────┴──────────┐
          │      MAG Core       │
          │ (Storage | Search | │
          │  Scoring | Embedder)│
          └─────────────────────┘
```

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-05-04 | Defer ingestion hook fixes | Log watcher in Phase 3 replaces them entirely |
| 2026-05-04 | 3-method harness trait | Unstructured traces avoid premature event typing |
| 2026-05-04 | AMA-Bench SWE only | Most relevant domain; avoids boiling the ocean |
| 2026-05-04 | MemoryArena one env only | Learn loop pattern without full integration burden |
| 2026-05-04 | No LiteLLM proxy | Env vars sufficient for Phase 2 scope |
| 2026-05-04 | JSON traces before TUI | Traces must exist before they can be displayed |
| 2026-05-04 | Active injection in Phase 2b | MemoryArena is the only benchmark that tests it |
| 2026-05-04 | Checkpoint gates between phases | Prevent building on unvalidated foundations |

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| #323 fix changes LoCoMo scores | Medium | High | Revert if >5pp regression |
| MemoryArena too slow via MCP | Medium | Medium | Fall back to direct SQLite API for benchmark runs |
| AMA-Bench doesn't translate to Claude Code | Medium | High | Treat as one signal; Phase 3 dogfooding is the real validation |
| Active injection hurts more than helps | Medium | Medium | A/B test passive vs. active in MemoryArena; keep the winner |
| Phase 2b harder than expected | Medium | Medium | Scope is one environment only; can cut to passive retrieval only |

## Open Questions

1. Exact Claude Code log schema for Phase 3 watcher?
2. Which recall strategy (threshold/boundary/periodic) works best in MemoryArena?
3. Should Phase 2c changes be gated by `./scripts/bench.sh --gate` or by AMA-Bench score only?
