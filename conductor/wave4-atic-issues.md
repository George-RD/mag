# Wave 4: ATIC-Inspired Cognitive Refinement — Issue Concepts

> All issues are **research/investigation** — none should produce code without validating the premise first.
> Inspired by [ATIC: A Geometric Theory of Artificial Cognition (Muniz, 2026)](https://truthagi.ai).
>
> Reviewed through 3 adversarial (/dg) cycles and 2 simplification passes.

---

## Issue 0: Knowledge Graph Utility Audit

### Summary
Measure whether graph-based retrieval mechanisms (Phase 5 graph neighbor traversal AND Phase 5b entity tag expansion) contribute measurably to retrieval quality, or just add latency.

### Motivation
Before adding new cognitive layers (Issues 1–4), we need to validate the existing ones. MAG has two graph-adjacent mechanisms in the RRF pipeline:

| Mechanism | Code | Parameter | Effect |
|---|---|---|---|
| **Phase 5: Graph neighbor traversal** | `graph.rs` BFS | `GRAPH_NEIGHBOR_FACTOR=0.1` | Adds neighbors' scores at 10% weight |
| **Phase 5b: Entity tag expansion** | `entities.rs` | `ENTITY_EXPANSION_BOOST=1.15` | Boosts memories found via entity tag matching |

These are independent — setting `GRAPH_NEIGHBOR_FACTOR=0.0` disables Phase 5 but leaves 5b fully active. The audit must test both.

**Critical caveat:** LoCoMo benchmark data has sparse relationship graphs (documented in `scoring.rs:190`). A LoCoMo-only ablation would produce a confounded result — zero delta would mean "LoCoMo doesn't exercise the graph," not "graph doesn't help." The investigation **must include a production-database test** with dense relationships.

### Investigation Plan
1. **LoCoMo ablation (baseline).** Run 2-sample LoCoMo-10 in three configurations: (a) both active (current), (b) `GRAPH_NEIGHBOR_FACTOR=0.0` only, (c) both disabled (`GRAPH_NEIGHBOR_FACTOR=0.0` + `ENTITY_EXPANSION_BOOST=1.0`). Measure per-category delta. Expect minimal difference due to sparse graphs — this establishes the baseline, not the conclusion.
2. **Production-database test.** Replay a set of real queries against the author's MAG database (which has dense entity graphs from months of use). Requires a query log export or a manually curated query set. Compare the same three configurations. This is the authoritative test.
3. Inspect queries where graph mechanisms change result ranking — are those changes improvements?
4. Profile latency cost of graph traversal (BFS, max_hops) and entity expansion on the production database.
5. If positive contribution: document which query types and graph densities benefit, tune factors.
6. If no contribution: file follow-ups to remove or feature-gate each mechanism independently.

### Architecture Touchpoints
- `src/memory_core/scoring.rs:193` — `GRAPH_NEIGHBOR_FACTOR` constant
- `src/memory_core/scoring.rs:199` — `ENTITY_EXPANSION_BOOST` constant
- `src/memory_core/storage/sqlite/advanced.rs` — Phase 5 graph enrichment, entity expansion call
- `src/memory_core/storage/sqlite/graph.rs` — BFS traversal, `max_hops`
- `src/memory_core/storage/sqlite/entities.rs` — entity tag expansion

### Risk Assessment
**LOW** — Investigation only. Benchmark comparison with constant changes.

### Success Criteria
A definitive answer per mechanism: "Phase 5 graph traversal improves category X by Y% on production data" or "Entity expansion has no measurable impact and costs Z ms per query." Either answer informs the Wave 4 roadmap.

---

## Issue 1: Epistemic Health Signal (φ-inspired memory health composite)

### Summary
Add a composite "epistemic health" metric to MAG that monitors the representational quality of the memory store. Unlike `check_health` (db size, integrity, node count), this measures whether memories are *useful*.

### Motivation
MAG's current health check is infrastructure-only. It can't answer: Is coverage diverse? Are important domains decaying? Is the store drifting toward a single-topic echo chamber?

ATIC's φ(M) composites four signals into one health scalar. The MAG analogue:

| Component | How to Compute | When Low, Do This | Notes |
|---|---|---|---|
| **Topic diversity** (entropy) | Shannon entropy of tag frequency distribution | Diversify input — store memories across more topics | Primary coverage signal |
| **Temporal freshness** | Fraction of memories accessed in last N days | Run consolidation on stale clusters; flag for review | |
| **Graph connectivity** | Ratio of connected to isolated entity subgraphs | Trigger entity re-extraction on orphaned memories | See Prerequisites |
| **Abstention rate** | Query miss rate over recent window | Flag scoring thresholds for review; may indicate domain gap | See Prerequisites |

**Causal linkage warning:** Topic diversity and abstention rate are not independent — a narrow-topic store naturally produces more abstentions for out-of-domain queries. Including both in a weighted composite risks double-counting coverage problems. The investigation must either (a) orthogonalize the signals (e.g., measure abstention rate *within* well-covered topics only) or (b) drop one and use the other as the sole coverage indicator.

Note: embedding spread (variance of pairwise cosine distances) was considered as a fifth component but correlates strongly with topic diversity in practice. Issue 4 (Embedding Drift Monitor) will supply this signal if divergence is found.

### Prerequisites
1. **Issue 0 (KG Utility Audit)** — if graph connectivity is not contributing to retrieval, the "graph connectivity" component is meaningless and should be dropped from the composite.
2. **Abstention rate instrumentation** (sub-task of this issue, blocks the φ prototype):
   - Add a per-window query counter
   - Add a per-window abstention counter
   - Compute windowed miss rate

   Currently nothing counts queries or tracks miss rate. This instrumentation must land before abstention rate can be included as a φ component.

### Research Questions
- What's the right weighting for these components? (Needs empirical data from the author's own MAG instance)
- What thresholds indicate "healthy" vs "degrading"? (Need baseline measurements)
- Can topic diversity be computed efficiently via sampling rather than full scan?
- How to orthogonalize topic diversity and abstention rate, or should we pick one?

### Design Decision
Expose both: scalar composite for monitoring, structured per-component report for diagnosis. Implement as `maintain --action epistemic-health`.

### Investigation Plan
1. Compute each component on the author's MAG database to establish baseline distributions
2. Check whether components are correlated — especially topic diversity vs abstention rate (causal linkage) and topic diversity vs embedding spread
3. Propose weights based on variance contribution; drop or orthogonalize correlated signals
4. Prototype as `maintain --action epistemic-health` with structured output
5. If useful, add scalar composite and expose via daemon for periodic monitoring

### Risk Assessment
**NONE** — Purely additive. No changes to search or scoring. No regression risk.

---

## Issue 2: Scoring Ceiling Analysis (MAD-inspired)

### Summary
Determine whether MAG's scoring pipeline is a retrieval bottleneck, and if so, whether ATIC's MAD-inspired Gaussian confidence model addresses the failure modes. This is a **bottleneck identification** issue, not a scoring rewrite.

### Motivation
MAG is at 90.1% on LoCoMo-10. Before proposing any scoring changes, we need to answer: **where is the remaining 9.9% lost?**

Possible ceilings:
1. **Retrieval** — the right memories aren't in the candidate set (FTS5/vector recall gaps)
2. **Scoring** — the right memories are retrieved but ranked incorrectly (multiplicative factor shape)
3. **Abstention** — the system correctly abstains but the benchmark counts it as a miss
4. **Data** — the benchmark memories weren't stored with enough signal

This issue investigates ceiling #2 specifically. Ceilings #1, #3, and #4 are diagnosed in Step 1 but are out of scope — findings for those categories should be filed as separate issues.

ATIC's MAD model (Gaussian confidence `exp(-d²/2τ²)` with domain-adaptive τ²) is a specific hypothesis about ceiling #2: that multiplicative scoring punishes near-misses too aggressively and doesn't penalize far-misses aggressively enough. This hypothesis is only worth testing if scoring shape is actually where the 9.9% is lost. For MAG, "domain" = EventType as the primary axis (22 types, each with distinct type_weight and TTL); tag clusters are a possible extension but require unsupervised clustering at query time — defer unless EventType proves insufficient.

### Investigation Plan
1. **Bottleneck identification.** Run LoCoMo-10 (2-sample) with `opts.explain=true` to get per-candidate scoring breakdowns (`vec_sim`, `fts_rank`, `rrf_score`, `dual_match`, `adaptive_dual_boost` — already wired through `fuse_refine_and_output`). For each missed query, classify: was the correct memory in the candidate set but ranked wrong (scoring ceiling), or not retrieved at all (retrieval ceiling)?
2. **Factor decomposition.** For scoring-ceiling misses, analyze the explain data to identify which factor(s) cause the mis-ranking (type_weight, time_decay, word_overlap, etc.).
3. **Gaussian simulation.** Compute what MAD-style Gaussian decay would produce for the same queries. Does it re-rank correctly?
4. **A/B benchmark.** If Step 3 is promising, implement as feature-flagged parallel scoring mode. Run `bench.sh --gate` comparison.

### Research Questions
- What fraction of LoCoMo misses are scoring-shape vs retrieval-gap? (This determines whether the issue matters at all)
- Does Gaussian decay help or hurt for keyword-intent queries? (Intent classification already exists — could apply different curves per intent)
- Should MAG have a τ² floor of 0.05 to prevent overconfident scoring on well-represented domains?

### Risk Assessment
**HIGH** for implementation (touches core scoring pipeline, every change risks regression). **LOW** for investigation (Steps 1–3 are read-only analysis).

### Gate
If Step 1 shows <30% of misses are scoring-ceiling, close this issue — the bottleneck is elsewhere. File separate issues for non-scoring ceiling categories identified during the analysis.

---

## Issue 3: Entropy-Weighted Memory TTL

### Summary
Investigate whether session diversity at store time correlates with memory longevity/usefulness, as ATIC's Law of Epistemic Validity (`T_exp ∝ H(input)`) predicts. The law states that knowledge expiry is proportional to the entropy of the input that created it — diverse contexts produce longer-lived knowledge.

### Motivating Example
`UserPreference` memories have no TTL — they live forever. But a user preference recorded during a narrow, single-topic debugging session ("always use debug logging") may be far more parochial than one recorded during a broad architectural review ("prefer composition over inheritance"). Both get `None` TTL today. The diversity of the context that produced a memory should influence how long it persists.

### Prerequisite: Data Validation
This issue is gated on a retrospective analysis. Before any code is written:

1. **Compute session diversity** for existing memories in the author's MAG database. For each memory, query all memories sharing the same `session_id`, collect their tag arrays, and compute a diversity metric over the combined tag distribution.

   **Small-sample guard:** Shannon entropy is numerically unstable for sessions with fewer than ~20 memories (the difference between 3 and 10 tags is dominated by sample size, not genuine diversity). Sessions below this threshold must be excluded from the analysis. Use normalized entropy (`H / log(n)`) to control for sample size, or use a simpler proxy (unique tag count).

2. **Correlate with utility.** Define utility as **days since `last_accessed_at`** (lower = more recently useful). Use this rather than raw `access_count` to avoid **survivorship bias** — `consolidate()` deletes memories with `access_count=0`, so swept memories vanish from the dataset and would attenuate any correlation.

3. **Threshold check.** If Pearson correlation between session diversity and memory utility is < 0.3 (a standard social-science convention for weak-to-moderate effect size; below this the relationship is too noisy to build a TTL modifier on), close this issue — the hypothesis doesn't hold for MAG's use case.

### If Validated — Investigation Plan
4. Prototype diversity computation at store time (metadata field: `session_diversity`)
5. Implement TTL modifier: `effective_ttl = base_ttl × (1 + diversity_bonus)` where `diversity_bonus` scales with session diversity
6. Test sensitivity on LoCoMo with artificially varied TTLs

### Architecture Touchpoints
- `src/memory_core/domain.rs:112` — `default_ttl()` per EventType
- `src/memory_core/storage/sqlite/lifecycle.rs` — `sweep_expired()`
- `src/memory_core/storage/sqlite/session.rs` — session tracking (diversity source)

### Research Questions
- Is normalized entropy (`H/log(n)`) the right diversity metric, or is a simpler proxy sufficient? (e.g., unique tag count, entity connection count)
- What's the minimum session size for stable diversity measurement? (Empirically determine from the author's database)

### Risk Assessment
**MEDIUM** — TTL changes affect lifecycle, not scoring. But premature implementation without data validation would add complexity for no gain.

---

## Issue 4: Daemon Medium Loop — Embedding Drift Monitor

### Summary
Add a single, concrete medium-frequency background task to MAG's daemon mode: **embedding drift detection** on a rolling window.

### Motivation
ATIC identifies three temporal feedback loops. MAG has the fast loop (per-query search pipeline) and slow loop (TTL sweep, consolidation). The missing medium loop sits between them — periodic self-assessment without waiting for a query to trigger it.

This issue scopes to **one falsifiable mechanism**: detecting whether the embedding distribution of recent memories is drifting away from the historical baseline.

### The Mechanism
At a configurable interval (see Research Questions), the daemon:
1. Samples K recent embeddings (last 100 memories) and K historical embeddings (random sample from full store)
2. Computes the centroid of each sample
3. Measures cosine distance between centroids
4. If drift > threshold (calibrated from empirical data), logs a warning with the drifting dimensions

This is a lightweight read-only operation. MAG uses WAL mode and a round-robin reader pool (`conn_pool.rs`, `DEFAULT_READER_COUNT=4`). Read-only queries don't block writers at the SQLite level; the only contention is brief `MutexGuard` acquisition on one of 4 reader slots — negligible in practice, as the drift check holds a reader for ~100ms while iterating embeddings.

### Investigation Plan
1. Instrument daemon to log embedding centroid snapshots over 1 week of real usage
2. Analyze: does drift correlate with any observable quality change? (abstention rate, retrieval accuracy)
3. Calibrate both the **interval** (candidate range: 1–10 minutes) and **threshold** (what drift magnitude is concerning) from empirical data
4. If validated, ship as a daemon background task with calibrated defaults

### Architecture Touchpoints
- `src/daemon.rs` — daemon mode HTTP server, add periodic task
- `src/memory_core/storage/sqlite/embedding_codec.rs` — embedding decode, `dot_product`
- `src/memory_core/storage/sqlite/conn_pool.rs` — reader pool, `MutexGuard<Connection>`

### Research Questions
- What sample size K gives stable centroid estimates? (100 may be too few for sparse stores)
- Is centroid drift the right metric, or is distribution spread (variance) more informative?
- What interval balances freshness with overhead? (Candidate range: 1–10 minutes)
- Should the daemon also track per-EventType drift (semantic memories drifting differently from episodic)?

### Risk Assessment
**LOW** — Read-only background operation with negligible mutex contention.

### Future Extensions (separate issues)
- Write mutations (proactive consolidation) — only after proving read-only loop is stable
- Multiple medium-loop signals (graph health, domain competence cache) — only after drift monitor is validated
- Cross-reference: supplies embedding spread data to Issue 1's φ composite if needed
