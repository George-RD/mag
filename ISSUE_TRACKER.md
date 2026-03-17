# Issue Tracker — Orchestration State

Last updated: 2026-03-17
LoCoMo word-overlap (2-sample): **75.3%** (was 74.4% at session start)
Evidence Recall: **77.7%**

## Completed This Session

| Issue | Title | PR | Result |
|-------|-------|----|--------|
| #42 | Fix LoCoMo paper citation | #48 | Merged — corrected arxiv link |
| #44 | CI feature flag matrix | #47 | Merged — 3-way matrix (all/none/default) |
| #43 | Cache invalidation tests | #50 | Merged — 5 integration tests |
| #39 | Query expansion (synonyms) | #49 | Merged — 50+ synonym groups, +0.9pp overall |
| #41 | Re-enable graph enrichment | #52 | Merged — GRAPH_NEIGHBOR_FACTOR=0.1, no regression |
| #6/#40 | Tune intent classification | #51 | Merged — per-intent multipliers, Single-Hop D->C |
| #37 | Temporal fact reconciliation | #54 | Merged — UserFact/Reminder supersession, entity_id scoping |
| #38 | End-to-end LLM evaluation | #55 | Merged — E2E word-overlap mode, adversarial 98.6% |

## Benchmark After Session

| Category | Start | End | Delta |
|----------|-------|-----|-------|
| Single-Hop QA | 61.4% (D) | 60.0% (C) | -1.4pp (grade up) |
| Temporal | 87.8% (B) | 87.6% (B) | -0.2pp |
| Multi-Hop | 43.7% (D) | 43.7% (D) | 0 |
| Open-Domain | 76.5% (B) | 78.4% (B) | +1.9pp |
| Adversarial | 72.6% (C) | 74.4% (C) | +1.8pp |
| **Overall** | **74.4%** | **75.3%** | **+0.9pp** |

## E2E Benchmark (2-sample, gpt-4o-mini)

| Category | E2E | Retrieval | AutoMem |
|----------|-----|-----------|---------|
| Single-Hop | 25.0% | 60.0% | 79.8% |
| Temporal | 49.3% | 87.6% | 85.1% |
| Multi-Hop | 5.8% | 43.7% | 50.0% |
| Open-Domain | 54.1% | 78.4% | 95.8% |
| Adversarial | **98.6%** | 74.4% | 100.0% |
| **Overall** | **57.3%** | **75.3%** | **90.5%** |

Key insight: AutoMem gap is in retrieval quality, not evaluation methodology. Adversarial near-perfect with LLM.

## Remaining Open Issues (6)

### Next Wave (research needed)

| Issue | Title | Status | Notes |
|-------|-------|--------|-------|
| #8 | Evidence pack assembly | backlog | Post-retrieval clustering |

### Future/Backlog

| Issue | Title | Status |
|-------|-------|--------|
| #10 | Wikipedia-scale benchmark | backlog |
| #7 | Memory architecture spectrum | backlog |
| #5 | omega-memory paid features | backlog |
| #4 | AutoMem augmentation | backlog |
| #3 | Fine-tuned embeddings | backlog |
