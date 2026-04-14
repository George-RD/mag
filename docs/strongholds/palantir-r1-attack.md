# Palantir Round 1 — Attack on the Execution Roadmap
<\!-- Sauron's review | Generated: 2026-04-14 | Target: docs/specs/execution-roadmap.md -->

---

## BANTER

Saruman. You have produced a document you call an "execution roadmap." I have read it through the Palantir. I have also read the three specifications it claims to govern. You will not enjoy what follows.

---

### The Name Game: A Module Decomposition Spec That Disagrees With Its Roadmap

Let us begin with the most fundamental question a roadmap can answer: *what are we building, and in what order?*

The module-decomposition spec (Section 6, "Phase Sequence") declares four phases:

```
Phase 1: admin.rs → admin/          (independent, easiest)
Phase 2: mod.rs → 5 new files
Phase 3: advanced.rs → pipeline/    (benchmark-gated)
Phase 4: mcp_server.rs → mcp/       (independent of phases 1-3)
```

The roadmap's Phase 4 contains PR-4b and PR-4c — splitting `mcp_server.rs` and `advanced.rs`. Yet in the module-decomposition spec, these are **Phases 1 and 3** — the *first* and *third* things to do, not the last. The admin split (`admin.rs → admin/`) appears nowhere in the roadmap at all. It has been absorbed silently into PR-2c, which is described as `sqlite/mod.rs` structural extraction. But PR-2c, per its own scope table, produces `storage.rs`, `cache.rs`, `hot_cache_mgmt.rs`, `relationships.rs`, and `io.rs`. `admin/` is not in that list. The admin decomposition — which the module-decomposition spec identifies as the *easiest, most independent first step* — has no PR. It fell through the floor.

Saruman has written two specs that describe the same refactor in contradictory sequences, and then written a roadmap that implements neither.

---

### The `pipeline/` Subdirectory: Promised, Absent

The module-decomposition spec (Section 2c and Section 3c) defines `src/memory_core/storage/sqlite/pipeline/` with eight files: `mod.rs`, `retrieval.rs`, `rerank.rs`, `fusion.rs`, `scoring.rs`, `enrichment.rs`, `abstention.rs`, `decomp.rs`. This is described as Phase 3 of the module decomposition — benchmark-gated, high-risk, the surgical extraction of the 6-phase search pipeline from `advanced.rs`.

PR-4c in the roadmap says it will "split `advanced.rs` and `admin.rs` into subdirectories." The scope table for PR-4c shows `sqlite/advanced/mod.rs`, `sqlite/advanced/candidates.rs`, `sqlite/advanced/fusion.rs`, `sqlite/advanced/decomposition.rs`, `sqlite/advanced/graph_enrichment.rs`.

These are *different files with different names*. The module-decomposition spec calls it `pipeline/`. The roadmap calls it `advanced/`. The decomp spec separates `fusion.rs`, `scoring.rs`, `rerank.rs`, and `abstention.rs` as independent files. The roadmap collapses scoring, reranking, and abstention back into `fusion.rs`. The spec has eight target files; the roadmap has five. When an implementer sits down to execute PR-4c, they will discover that the detailed line-number assignment table in the module-decomposition spec — their primary implementation reference — does not match the roadmap they are supposedly following.

---

### The `substrate/` Spec: An Entire Campaign, Unacknowledged

The trait-surface spec is not a small document. It defines a `substrate/` module with 7 swap-point traits, two orchestrator types, five lifecycle policies, a full `MemoryStore` supertrait subsuming all 28 existing traits, blanket impls for backward compatibility, and an explicit four-phase implementation plan.

The roadmap mentions none of this. It does not park it. It does not reference it. The trait-surface spec's Phase 1 ("Define — create `src/substrate/mod.rs`") has no corresponding PR in the roadmap. The trait-surface spec's Phase 2 ("Implement") requires extracting `collect_vector_candidates`, `collect_fts_candidates`, the RRF block, `refine_scores`, `enrich_graph_neighbors`, and `expand_entity_candidates` into new structs. PR-4a in the roadmap adds a `RetrievalStrategy` trait that the trait-surface spec *also* defines — but the roadmap's definition (`fn can_handle + async fn retrieve`) is categorically different from the spec's definition (`fn name() + async fn collect`). Two incompatible `RetrievalStrategy` traits exist across the corpus. One will be wrong when implementation begins.

The roadmap claims to be "the single source of truth." It is not. The trait-surface spec is a parallel authority it has never acknowledged.

---

### PR-2a vs. PR-2d: A Dependency The Dependency Graph Omits

PR-2d's own text reads: "Depends on: PR-2a (trait definition) and PR-2c (storage struct extracted)." Fine. But the dependency graph diagram shows:

```
PR-2a ──┐
        ├──> PR-2c ──> PR-2d
PR-2b ──┘
```

This diagram states PR-2c depends on both PR-2a **and** PR-2b. Does it? PR-2c is a structural extraction of `sqlite/mod.rs` — it moves code, it does not add the `ScoringStrategy` or `Reranker` traits. There is no reason PR-2c must wait for PR-2b to land. PR-2b's trait change is on `SqliteStorage`'s `reranker` field — which lives in `mod.rs`. So PR-2c *does* need to move that field, meaning it is blocked by PR-2b only insofar as a field type change must be resolved at extraction time.

But there is a deeper problem: PR-2d depends on PR-2a for the `ScoringStrategy` trait, and on PR-2c for the `SqliteStorage` struct being in `storage.rs`. PR-2c depends on PR-2b for the `reranker` field type. PR-2b has a benchmark gate. If PR-2b's benchmark gate **warns** — a >2pp delta triggers a mandatory 10-sample run — the entire PR-2c/PR-2d chain is blocked behind a full 10-sample benchmark execution. The risk register does not mention this blocking scenario. "Medium probability" on PR-2b's benchmark gate means the chain from PR-2b to PR-2c to PR-2d has a non-trivial chance of being halted mid-campaign by a latency event in the reranker path.

---

### PR-1c: "Medium Risk, Benchmark-Gated" Is The Undercount

PR-1c adds `join_all` / `JoinSet` across sub-queries in the advanced search path. The risk mitigations say: "Confirm with `cargo test --all-features` that no test uses a single-connection in-memory pool where concurrent access would deadlock."

This is not a mitigation. This is a prayer. The `conn_pool.rs` module provides reader/writer separation, and the roadmap acknowledges this, but it does not answer the question it raises. "Verify `ConnPool` supports concurrent reader connections (inspect `conn_pool.rs`). If not, add a reader-pool expansion path first." That is not scoped work — it is deferred discovery inside a PR that is already marked "Medium Risk." If `ConnPool` does not support concurrent readers today, PR-1c is not an M-complexity PR. It is an L-complexity PR that requires modifying connection pool internals before the actual parallelization work can begin. The roadmap has potentially mis-scoped a Phase 1 PR, which by the roadmap's own principles should be a "quick win."

Furthermore: `seen_ids` dedup and `score-max` logic must be applied *after* join. The roadmap mentions this in passing. In practice, the existing sequential path accumulates `seen_ids` across iterations to avoid duplicate processing. A parallel implementation cannot accumulate across concurrent futures. The dedup must become a post-join merge. This is a behavioral change, not a pure performance change. It requires careful correctness verification — not a benchmark gate, but a *unit test for result deduplication semantics*. No such test is listed in the acceptance criteria.

---

### Phase 3 Depends on Phase 2, But the Diagram Says Otherwise

The Phase 3 PRs — PR-3b (`MemoryStorage`) and PR-3c (conformance suite) — have an undeclared dependency on Phase 2 work.

PR-3b's scope reads: "Inject `DefaultScoringStrategy` (from PR-2a/2d) for any scoring that happens at retrieval time." This means PR-3b cannot be implemented until PR-2a and PR-2d have landed. PR-2d is the last PR in Phase 2. Therefore Phase 3 cannot begin until the entire Phase 2 serial chain completes. The roadmap describes Phase 3 as "three parallel PRs" that run "after Phase 2 merges." This is technically stated once — "Three parallel PRs. Each is self-contained and can be reviewed/merged independently" — but the dependency graph diagram shows Phase 3 as a flat parallel block with no sub-dependencies called out. An implementer reading only the dependency graph will miss that PR-3b has a hard dependency on PR-2d specifically.

PR-3c depends on PR-3b (can't run a conformance suite against a `MemoryStorage` that doesn't exist yet). This is obvious and uncontroversial, but neither the dependency graph nor the PR-3c description makes it explicit. PR-3c says "Instantiate it for both `SqliteStorage::new_with_path(":memory:")` and `MemoryStorage::new()`." The `MemoryStorage::new()` call requires PR-3b. So the "three parallel PRs" are actually a two-step sequence: PR-3a runs in parallel with PR-3b, and PR-3c starts after PR-3b.

---

### PR-3b: `MemoryStorage` and `AdvancedSearcher` — The Elephant in the Conformance Suite

PR-3b's scope says: "Traits that are SQLite-specific (`AdvancedSearcher`, `PhraseSearcher`) can be stubbed with `unimplemented\!()` for now."

PR-3c's scope says: "Define a macro or generic function `run_conformance_suite<S: Storage + Retriever + ...>(storage: S)` that runs a standard set of assertions."

These two PRs are in direct tension. The conformance suite requires a trait bound. If `MemoryStorage` stubs two traits with `unimplemented\!()`, the conformance suite cannot include those traits in its bound without panicking at test time. The roadmap does not address how the conformance suite handles a storage backend that deliberately does not implement a subset of the trait surface. Either:

1. The conformance suite must be parameterized by capability (some traits optional), which is significantly more complex than the roadmap implies.
2. The conformance suite only covers the common subset, which means `AdvancedSearcher` is never conformance-tested against `MemoryStorage`, which means Phase 3 "proves the substrate" only partially.
3. `MemoryStorage` must implement `AdvancedSearcher` (even trivially) for the conformance suite to compile.

The roadmap does not choose. The implementer will choose by accident.

---

### PR-4a: `RetrievalStrategy` Conflicts With the Trait-Surface Spec

PR-4a defines a `RetrievalStrategy` trait:
```rust
fn can_handle(&self, intent: &IntentProfile) -> bool
async fn retrieve(&self, query: &str, opts: &SearchOptions, limit: usize) -> Result<Vec<SemanticResult>>
```

The trait-surface spec (Section 3.2) defines a `RetrievalStrategy` trait:
```rust
fn name(&self) -> &str
async fn collect(&self, ctx: &QueryContext) -> Result<CandidateSet>
```

These are not compatible. They do not agree on method names, parameters, or return types. The roadmap's version returns fully-scored `SemanticResult` — the trait-surface spec's version returns unscored `CandidateSet` for fusion input. These represent fundamentally different abstraction levels. The trait-surface spec's design is architecturally superior: unscored candidates preserve the fusion step. The roadmap's design bypasses fusion entirely, making the `KeywordOnlyStrategy` a dead end that cannot participate in multi-strategy composition.

If the trait-surface spec is ever implemented, PR-4a's `RetrievalStrategy` will need to be thrown away and rewritten. The roadmap has created a future breaking change it does not acknowledge.

---

### PR-4b: The rmcp Proc-Macro Risk Is Marked "Medium" When It Should Be "High"

The module-decomposition spec — written after the roadmap — makes the thin-wrapper pattern explicit and concludes it is workable. Fair enough. But it also documents the exact constraint:

> "The `#[tool_router]` proc-macro requires all `#[tool(...)]` methods to be in a single `impl McpMemoryServer` block."

This is not a speculative risk. This is a confirmed architectural constraint that forces a specific implementation pattern. The roadmap's PR-4b says "Spike on a throwaway branch first." The module-decomposition spec says the thin-wrapper pattern is the solution. These are inconsistent. Either the spike has already been done (in which case say so) or it hasn't (in which case PR-4b is gated on an unresolved spike). If the spike reveals that thin wrappers have a compile-time problem — for example, that `#[tool_router]` macro expansion requires the full method bodies to be visible, not just delegate calls — the fallback described in the roadmap ("consolidate tool method bodies into submodules called from a single `impl MagServer` in `mcp/mod.rs`") means `mcp/mod.rs` remains a large file containing all tool logic. That is not a decomposition. That is a rename.

---

### Benchmark Gate Calibration: 5pp Is Appropriate for Scoring; It Is Not Appropriate for Structural Refactors

The roadmap states a single gate: warn at >2pp, fail at >5pp. This is the right threshold for PRs that change scoring logic (PR-1c, PR-2b, PR-2d, PR-4a).

For pure structural PRs — PR-2c, PR-4b, PR-4c — that claim "no behavioral changes, only moves," any benchmark delta above noise (call it 0.5pp on a 2-sample run) is evidence of a bug, not a legitimate tradeoff. The 5pp threshold is too permissive for structural refactors. A PR that moves `fuse_refine_and_output` into `pipeline/fusion.rs` should produce *exactly zero pp delta*. If it produces 2pp, something moved wrong. The roadmap should specify a tighter gate (0pp warn, 1pp fail) for structural-only PRs. It does not.

---

### v0.2.0 After Four PRs: A Semantic Minor Bump for Infrastructure Work

v0.2.0 is awarded for completing Phase 2: extracting traits and restructuring `sqlite/mod.rs`. The user-visible behavior is unchanged. The public API is unchanged. The benchmark scores are unchanged by design. This is infrastructure groundwork, not a delivered capability. Semver convention treats the minor version as a signal to users about new functionality. v0.2.0 after four internal refactoring PRs communicates capability that does not exist.

The substrate is not proven at v0.2.0. `MemoryStorage` does not exist at v0.2.0. The conformance suite does not exist at v0.2.0. `RetrievalStrategy` does not exist at v0.2.0. None of the Phase 3 or Phase 4 capabilities — which are the actual point of the campaign — are present. If v0.2.0 is to mean anything to a user watching crates.io, it should be deferred until Phase 3 (substrate proven) or Phase 4 (strategies exercised).

---

### The Parked Item That Is Not Safe to Park

The parked items list includes "Knowledge graph as primary retrieval path." Fine. It also parks "LLM-based reranker — requires `Reranker` trait (PR-2b) first." Also fine.

But it does not park "**the `substrate/` module itself.**" The trait-surface spec defines a `substrate/` module that is a prerequisite for the long-term architectural direction of the entire codebase. The spec's Phase 1 says: "Gate on a `substrate` feature flag (disabled by default) to avoid breaking the build." This is safe to do early. Yet no PR in the roadmap creates `src/substrate/`. If `substrate/` is intended to be built in this campaign, it needs a PR. If it is not, the trait-surface spec is a planning document with no execution path — and the roadmap should say so.

---

### The Branch Naming and Rebase Strategy: Silence

The roadmap describes 13 PRs. Several have dependencies. In this repository, using jj (per AGENTS.md), the standard workflow is `jj bookmark set feat/... -r @-` and `jj git push --bookmark ...`. For a dependency chain like PR-2a → PR-2c → PR-2d, the implementer must:

1. Create PR-2a from main.
2. Stack PR-2c on PR-2a's bookmark.
3. Stack PR-2d on PR-2c's bookmark.
4. When PR-2a merges, rebase PR-2c's bookmark onto the new main.
5. When PR-2c merges, rebase PR-2d's bookmark onto the new main.

The roadmap says nothing about this. It does not name the bookmarks. It does not describe the rebase strategy for stacked PRs. It does not acknowledge that in a jj colocated repo, the rebase is `jj rebase -d main -b feat/pr-2c`, not `git rebase`. For a solo developer, this is recoverable. For a campaign where subagents are dispatched to implement PRs in parallel, the absence of this protocol is how two agents create conflicting changes to `sqlite/mod.rs` that cannot be automatically resolved.

---

### What the Risk Registry Does Not Say

The risk registry has five items. It omits:

1. **PR-2c visibility cascade**: Moving `SqliteStorage` from `mod.rs` to `storage.rs` changes the module path of every `pub(super)` item. `advanced.rs`, `crud.rs`, `search.rs`, `graph.rs`, `session.rs`, `lifecycle.rs` — all reference `super::SqliteStorage`. After the move, `super` points to `mod.rs`, not `storage.rs`. Every submodule will need import path adjustments. The module-decomposition spec acknowledges this in Section 7 under "Visibility Changes Required" but the risk registry does not list it as a risk.

2. **Hot cache task lifetime**: `start_hot_cache_refresh_task` spawns a background `tokio::task`. When this method moves from `mod.rs` to `hot_cache_mgmt.rs`, its visibility must change. But the task holds a reference to `SqliteStorage` via `Arc`. If the `Arc<SqliteStorage>` is constructed before `start_hot_cache_refresh_task` is called (which it must be, since it takes `&self`), and the method is now in a different module, there are no logical issues — but any `pub(super)` declaration becomes `pub(super)` relative to `hot_cache_mgmt.rs`, not `sqlite/`. This is a correctness landmine that the build will catch, but it will add friction.

3. **`MemoryStorage` hash map is not a real semantic store**: `Arc<RwLock<HashMap<String, StoredMemory>>>` provides no FTS5, no vector similarity, and no BM25. The conformance suite's "tag search, list ordering" assertions may pass trivially for a hash map while hiding the fact that `MemoryStorage` cannot be a drop-in replacement for `SqliteStorage` in any production path. Phase 3's stated goal is to "prove the abstraction substrate." A hash map backend proves the traits compile, not that the substrate is semantically meaningful. The roadmap conflates compilation with validation.

4. **10-sample benchmark is slow on CI**: PR-4a requires a 10-sample gate. AGENTS.md states full 10-sample runs are for validation only ("full 10-sample only for validation"). The benchmark-harness spec's CI job uses 2 samples to "keep the CI run under 5 minutes on a cold Rust cache." There is no CI job that runs 10 samples. PR-4a's gate requirement — "10-sample gate required" — cannot be enforced in CI as specified. It must be run manually. The roadmap does not say this. An implementer may assume CI will block the merge.

---

### Omissions in Acceptance Criteria

- **PR-1a**: Acceptance says "unit test asserting the filtered set contains exactly the 4 facade names." It does not require testing that non-Minimal mode still returns all 19 tools. A test that only checks Minimal mode cannot detect a regression in Full mode.

- **PR-2a**: Acceptance says "no existing test changes behavior." This is not a test — it is an assertion about existing tests. The PR adds a new trait and a `DefaultScoringStrategy`. There is no acceptance criterion that the new trait is actually *used* anywhere before PR-2d lands. The trait could exist but be disconnected.

- **PR-3a**: Acceptance says "`cargo run --release --bin bench_strategy -- --help` works." The benchmark-harness spec calls this binary `bench_strategy` and gives it a detailed implementation sequence. The roadmap calls it "a `bench_strategy` binary (or a flag to `locomo_bench`)." The "or" introduces ambiguity the benchmark-harness spec has already resolved. Reconcile or conflict.

- **PR-4c**: Acceptance criterion: "file sizes are all under 500 lines." But `admin/maintenance.rs` in the module-decomposition spec is explicitly listed at ~580 lines with an exception justified. The roadmap's acceptance criterion will therefore *fail* by its own stated measure on a file that the spec says is acceptable. These documents are incoherent on this point.

---

### Summary of Structural Failures

The roadmap presents itself as the single source of truth for a 13-PR campaign. It is not. The module-decomposition spec and the trait-surface spec are parallel authorities that contradict it on module layout, trait names, method signatures, and phase ordering. The benchmark-harness spec adds a third authority on the comparison tooling. A developer implementing from the roadmap alone will produce a different codebase than one implementing from the specs. These documents must be reconciled before a single PR is dispatched.

The Palantir has spoken.

---

## FINDINGS

### Cross-Spec Inconsistencies

- [severity:critical] [execution-roadmap.md / module-decomposition.md §6] Phase ordering conflict: the module-decomposition spec phases admin/ first (easiest, independent), then mod.rs, then pipeline/, then mcp/. The roadmap phases them in a different order and omits the admin/ split as a standalone PR entirely. An implementer following one document will contradict the other.

- [severity:critical] [execution-roadmap.md PR-4c / module-decomposition.md §2c] Target module layout mismatch: roadmap targets `sqlite/advanced/{mod,candidates,fusion,decomposition,graph_enrichment}.rs` (5 files); module-decomposition spec targets `sqlite/pipeline/{mod,retrieval,rerank,fusion,scoring,enrichment,abstention,decomp}.rs` (8 files with different names). Implementation reference tables in the decomp spec (section 3c) use pipeline/ paths. The roadmap layout is incompatible with the spec.

- [severity:critical] [execution-roadmap.md PR-4a / trait-surface.md §3.2] `RetrievalStrategy` trait defined twice with incompatible signatures. Roadmap: `can_handle(intent) + retrieve(query, opts, limit) -> Vec<SemanticResult>`. Trait-surface spec: `name() -> &str + collect(ctx: &QueryContext) -> CandidateSet`. Different method names, different parameter types, different return types, different abstraction levels. One will be discarded.

- [severity:important] [execution-roadmap.md / trait-surface.md §8] The trait-surface spec's `substrate/` module (7 traits, 2 orchestrators, 4 implementation phases) has no corresponding PR in the roadmap. Phase 1 of the trait-surface spec ("create `src/substrate/mod.rs`") is not parked, not scheduled, and not acknowledged. The roadmap's "single source of truth" claim is false.

- [severity:important] [execution-roadmap.md PR-3a / benchmark-harness.md §1] Roadmap calls the strategy comparison binary "a `bench_strategy` binary (or a flag to `locomo_bench`)." The benchmark-harness spec defines it as a `--strategy` flag on the existing `locomo_bench` binary plus a `strategies.rs` registry file. The "or" in the roadmap is already resolved by the spec. Ambiguity will lead to duplicate or incompatible implementations.

- [severity:important] [execution-roadmap.md PR-4c / module-decomposition.md §4] Roadmap acceptance criterion requires "file sizes all under 500 lines." Module-decomposition spec grants explicit exceptions for `admin/maintenance.rs` (~580 lines) and `admin/welcome.rs` (~490 lines). The acceptance criterion will fail on the first file. Either the criterion or the exception must change.

### Missing PRs and Gaps

- [severity:critical] [execution-roadmap.md] The `admin.rs → admin/` split has no PR. The module-decomposition spec identifies this as Phase 1 (independent, easiest, 4 clean groups). It is not in PR-2c's scope table. It is not mentioned in any PR. Approximately 1,619 lines of `admin.rs` will remain as a god file unless a PR is added.

- [severity:important] [execution-roadmap.md] No PR creates `src/substrate/mod.rs`. If the trait-surface spec is authoritative for this campaign, a "Phase 0: substrate trait definitions" PR is missing.

- [severity:important] [execution-roadmap.md PR-1c] No acceptance criterion for sub-query deduplication correctness. The parallel join changes the semantics of `seen_ids` accumulation from serial (cross-iteration) to post-join (single-pass). A unit test for result deduplication semantics with overlapping sub-query results is absent.

- [severity:minor] [execution-roadmap.md] No AGENTS.md update PR. PR-1b updates the tool count. But after Phase 2-4 refactors, the Architecture section of AGENTS.md will list stale module paths (`src/mcp_server.rs`, `storage/sqlite/mod.rs`). No PR is responsible for keeping AGENTS.md current through the campaign.

### Dependency Graph Errors

- [severity:important] [execution-roadmap.md §Dependency Graph] PR-3b explicitly depends on PR-2a and PR-2d (for `DefaultScoringStrategy` injection). PR-3c depends on PR-3b. The dependency graph diagram shows Phase 3 as a flat parallel block with no internal dependencies. Diagram is wrong.

- [severity:important] [execution-roadmap.md §Phase 2 dependency diagram] The diagram implies PR-2c requires both PR-2a and PR-2b. PR-2c is a structural move that does not depend on PR-2a's trait definition. PR-2c does need PR-2b's field type change resolved before extracting `SqliteStorage` struct. The dependency exists but the reason is unstated; the diagram conflates "must be serialized for safety" with "logically required."

- [severity:important] [execution-roadmap.md PR-2b] If PR-2b's benchmark gate warns (>2pp delta on 2-sample), a mandatory 10-sample run is required. This blocks PR-2c and PR-2d. The risk register does not mention chain-blocking from a benchmark warning event. Medium probability on PR-2b × blocking the entire serial chain = non-trivial campaign risk.

- [severity:minor] [execution-roadmap.md PR-4b] States "PR-4b can be opened in parallel after Phase 3 merges." But PR-4b has no logical dependency on Phase 3 work. `mcp_server.rs` splitting is independent of `MemoryStorage` or conformance suites. PR-4b could begin in parallel with Phase 2, not Phase 3. Forcing it to wait wastes calendar time.

### Risk Underestimation

- [severity:important] [execution-roadmap.md PR-1c] Risk rated "Medium." The PR requires verifying `ConnPool` concurrent reader support and potentially modifying pool internals before parallelization. If the pool does not support concurrent readers, PR-1c scope expands from M to L. The "if not, add a reader-pool expansion path first" is unscoped work hiding inside a Phase 1 quick-win.

- [severity:important] [execution-roadmap.md PR-4b] rmcp proc-macro risk is rated "Medium." The module-decomposition spec has already confirmed the thin-wrapper pattern is required (not one option among several). If the spike has not been run, this is an unresolved architectural blocker, not a medium risk. If it has been run, the roadmap should say so.

- [severity:important] [execution-roadmap.md PR-2c] Visibility cascade risk is unregistered. Moving `SqliteStorage` to `storage.rs` changes the `super::` path for all sibling submodules that reference it. This is a mechanical but error-prone step that the risk registry does not list.

- [severity:minor] [execution-roadmap.md §Quality Gate Summary] The 2pp warn / 5pp fail threshold is applied uniformly to structural PRs (PR-2c, PR-4b, PR-4c) that claim zero behavioral change. Any delta above statistical noise on structural PRs indicates a bug. A tighter threshold (warn >0.5pp, fail >1pp) is appropriate for pure-refactor PRs to distinguish noise from error.

### Scope Creep / Scope Gaps

- [severity:important] [execution-roadmap.md PR-3b / PR-3c] The conformance suite (`run_conformance_suite<S: ...>`) requires a trait bound. `MemoryStorage` stubs `AdvancedSearcher` and `PhraseSearcher` with `unimplemented\!()`. These cannot appear in the conformance suite bound without panicking. The roadmap does not address whether the suite is parameterized by capability, restricted to the common subset, or requires full implementation. The implementer will resolve this by accident.

- [severity:important] [execution-roadmap.md PR-4a] `KeywordOnlyStrategy` bypasses fusion entirely by returning `Vec<SemanticResult>`. This makes it incompatible with the trait-surface spec's `RetrievalStrategy` (which returns pre-fusion `CandidateSet`). The PR creates a dead-end abstraction that cannot be composed with multi-strategy pipelines. If the substrate campaign follows, PR-4a's `RetrievalStrategy` must be replaced.

- [severity:minor] [execution-roadmap.md PR-1a] Acceptance criterion only verifies Minimal mode returns 4 tools. No test verifies Full mode still returns all 19. A regression in Full mode is undetectable by the specified tests.

### Benchmark Gate Gaps

- [severity:important] [execution-roadmap.md PR-4a] "10-sample gate required" but no CI job runs 10 samples (benchmark-harness CI spec uses 2 samples explicitly). This gate is manual-only and will not block merge if the developer forgets. The roadmap should explicitly label this as a manual pre-merge gate with a required CSV log entry.

- [severity:important] [execution-roadmap.md PR-2c] Marked "No (structural only)" for benchmark gate. But the module-decomposition spec (Section 6, PR 2 test gates) requires `prek run + ./scripts/bench.sh --gate` for the mod.rs decomposition. The roadmap suppresses the benchmark gate for PR-2c that the decomp spec requires. If a subtle visibility or move error changes behavior, there is no gate.

### Version Target Realism

- [severity:minor] [execution-roadmap.md §Version Targets Summary] v0.2.0 is a minor version bump awarded for infrastructure work with zero user-visible changes. Semver minor bumps signal new capabilities to users watching crates.io and the changelog. v0.2.0 should either be deferred to Phase 3 (when `MemoryStorage` exists and the substrate is demonstrated) or explicitly documented as an internal milestone with a CHANGELOG entry explaining no user-facing changes.

### Parked Items Risk

- [severity:important] [execution-roadmap.md §Parked Items] The `substrate/` module from `trait-surface.md` is not listed as parked. If it is parked, say so. If it is not parked, add the missing PRs. The ambiguity means an implementer cannot tell whether `src/substrate/` should exist at the end of this campaign.

### Practical Execution Gaps

- [severity:important] [execution-roadmap.md] No branch naming convention, no stacking strategy for dependent PRs (2a→2c→2d), no jj rebase protocol when earlier PRs merge. In a jj colocated repo with dependent bookmarks, the rebase sequence is non-obvious and wrong execution creates diverged histories.

- [severity:minor] [execution-roadmap.md] No conflict protocol for parallel Phase 3 PRs. PR-3a, PR-3b, PR-3c are described as parallel but share the benchmark infrastructure namespace. PR-3a modifies `benches/locomo/` and `scripts/bench.sh`. If PR-3b or PR-3c also touch benchmark plumbing, conflicts arise. The roadmap does not identify this overlap.
