# Issue Tracker — Orchestration State

Last updated: 2026-03-17
Baseline LoCoMo word-overlap (10-sample): **74.9%** | 2-sample fast: **74.4%**

## Wave 3A: Quick Wins (in progress)

| Issue | Title | Status | PR | Notes |
|-------|-------|--------|---:|-------|
| #42 | Fix LoCoMo paper citation | 🔄 dispatched | — | Trivial docs fix |
| #44 | CI feature flag matrix | 🔄 dispatched | — | Add matrix strategy to ci.yml |
| #43 | Cache invalidation tests | 🔄 dispatched | — | Integration tests for store→update→query |
| #39 | Query expansion (synonyms) | 🔄 dispatched | — | Static synonym map + inject into build_fts5_query() |

## Wave 3B: Parameter Tuning (blocked on #39)

| Issue | Title | Status | Notes |
|-------|-------|--------|-------|
| #41 | Re-enable graph enrichment | ⏳ blocked | Grid search GRAPH_NEIGHBOR_FACTOR after #39 |
| #6/#40 | Tune intent classification + top-k | ⏳ blocked | Grid search multipliers after #39 |

## Wave 4: Medium Effort (future)

| Issue | Title | Status | Notes |
|-------|-------|--------|-------|
| #38 | End-to-end LLM evaluation | ⏳ backlog | New benchmark mode |
| #37 | Temporal fact reconciliation | ⏳ backlog | Research-first |
| #8 | Evidence pack assembly | ⏳ backlog | Post-retrieval clustering |

## Backlog

| Issue | Title | Status |
|-------|-------|--------|
| #10 | Wikipedia-scale benchmark | ⏳ backlog |
| #7 | Memory architecture spectrum | ⏳ backlog |
| #5 | omega-memory paid features | ⏳ backlog |
| #4 | AutoMem augmentation | ⏳ backlog |
| #3 | Fine-tuned embeddings | ⏳ backlog |
