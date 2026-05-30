# Issue Tracker — Orchestration State

Last updated: 2026-05-30
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

## Completed 2026-05-21

- Substrate Phase 3 search pipeline implementation
- MemoryStorage advanced_search and phrase_search implementation
- All substrate trait implementations:
  - RetrievalStrategy
  - FusionStrategy
  - Scorer
  - LifecyclePolicy
  - ConsolidationStrategy
  - IngestionPipeline

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

## Discovered During Development

| Issue | Title | Status | Notes |
|-------|-------|--------|-------|
| #N | Substrate `MemoryStore` trait lacks bulk-lookup by ID | open | `GraphNeighborScorer` clones HashMap for spawn_blocking; a `get_many` method would be more efficient |
| #N | `EmbedAndExtractPipeline` double-computes embeddings | resolved | Pipeline now passes pre-computed embedding via `store_with_embedding` (`ingestion_impl.rs`, `store_impl.rs`, `crud.rs`) |

## Remaining Open Issues (6)

> **Note:** The concrete substrate implementation is now complete. All substrate traits have been implemented and integrated.

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

## Adversarial Review Findings (2026-05-21)

### Critical
1. **Retrieval strategies run sequentially** — `SearchPipeline::search` uses a `for` loop, but doc comment claims concurrent execution (`tokio::join!` / `FuturesUnordered`). Performance regression and contract mismatch. (`orchestrators.rs`) — **FIXED**: now uses `futures::future::join_all`
2. **Explain-path panic risk** — `meta.as_object_mut().unwrap()` in `SearchPipeline::search` will panic if `candidate.result.metadata` is a JSON scalar or array instead of an object. (`orchestrators.rs`) — **FIXED**: replaced `unwrap()` with `if let Some`
3. **Candidate loss on scorer error** — `GraphNeighborScorer` and `EntityExpansionScorer` call `std::mem::take(candidates)` before fallible `store.*` calls. If the store returns an error, `?` propagates it but candidates are already emptied. (`enrichment_impl.rs`) — **FIXED**: uses `clone()` instead of `std::mem::take()`
4. **dry_run contract violation** — `CompactConsolidation` unconditionally calls `store.consolidate()` even when `dry_run = true`, which mutates the store. (`consolidation_impl.rs`) — **FIXED**: returns error when `dry_run = true`
5. **EmbedAndExtractPipeline steps incomplete** — Computes embedding then discards it; does not perform auto-supersession, dedup, entity extraction, or relationship creation as documented. (`ingestion_impl.rs`) — **NOTED**: documented with TODO; requires MemoryStore API extension

### Warnings
1. **RRF formula deviation** — `RrfFusion` multiplies raw signal score by reciprocal-rank weight, but trait docs define RRF without raw-score scaling. Deviates from standard RRF. (`fusion_impl.rs`) — **FIXED**: pure RRF, `existing.score += rrf_score`, `merged.score = rrf_score`
2. **Redundant query tokenization** — `SearchPipeline::search` never populates `ctx.query_tokens`, so every scorer independently re-tokenizes the query. (`orchestrators.rs`) — **FIXED**: pre-computes `ctx.query_tokens` once before scorer loop
3. **Cross-encoder length mismatch** — `CrossEncoderScorer` zips `ids` with `ce_scores` without verifying equal lengths. (`enrichment_impl.rs`) — **FIXED**: `bail!` on length mismatch before zip
4. **TTL malformed-data default** — `TtlExpirationPolicy::is_alive` returns `true` (alive) when `expires_at` is missing, non-string, or invalid RFC3339. Should arguably default to `false` for safety. (`lifecycle_impl.rs`) — **FIXED**: fail-closed, malformed/non-string/unparseable → `is_alive = false`

### Nits
1. **Unnecessary query_tokens clone** — `MultiFactorScorer` clones `query_tokens` HashSet when a shared reference would suffice. (`scoring_impl.rs`) — **FIXED**: borrows `ctx.query_tokens` instead of cloning
2. **Redundant id clone in WritePipeline** — `WritePipeline::ingest` clones `input.id` before moving the owned `input` into `WriteContext`; could move instead. (`orchestrators.rs`) — **FIXED**: uses `input.id.take()` instead of clone

## Adversarial Review Findings — Round 2 (2026-05-21)

### Critical
1. **EmbedAndExtractPipeline still incomplete** — (`ingestion_impl.rs`) — **FIXED**: delegates full store path (dedup/supersession/entity/relate) via `store_with_embedding`

### Warnings
1. **Cross-encoder length mismatch still present** — `CrossEncoderScorer` zips `ids` with `ce_scores` without verifying equal lengths. Unchanged from Round 1. (`enrichment_impl.rs`) — **FIXED**: `bail!` on length mismatch before zip
2. **Redundant query tokenization still present** — `SearchPipeline::search` never populates `ctx.query_tokens`, so every scorer independently re-tokenizes. Unchanged from Round 1. (`orchestrators.rs`) — **FIXED**: pre-computes `ctx.query_tokens` once before scorer loop

### Nits
1. **Redundant id clone still present** — `WritePipeline::ingest` clones `input.id` before moving the owned `input` into `WriteContext`. Unchanged from Round 1. (`orchestrators.rs`) — **FIXED**: uses `input.id.take()` instead of clone
