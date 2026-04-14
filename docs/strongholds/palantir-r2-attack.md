# Palantir Round 2 — Attack on the Reforged Execution Roadmap
<\!-- Sauron's review | Generated: 2026-04-14 | Target: docs/specs/execution-roadmap.md (reforged) -->

---

## BANTER

Saruman.

You were given thirteen issues. You have addressed most of them. I acknowledge this as one acknowledges the turning of a tide that has not yet reached the walls of Barad-dûr. It changes little.

Let us begin with what was fixed, because I will be accurate even when accuracy costs me something.

---

### What the Wizard Actually Repaired

The three critical failures of Round 1 are no longer critical. The `admin.rs → admin/` split now has its own PR — PR-1d — placed correctly in Phase 1, independent, well-scoped, with an ordered five-step execution sequence and explicit file size exceptions cross-referenced to `module-decomposition.md §4`. The acceptance criterion even reflects the exception: "file sizes are all under 500 lines (with `admin/maintenance.rs` ~580 and `admin/welcome.rs` ~490 exceptions per `module-decomposition.md §4`)." That is a correct fix.

The `pipeline/` versus `advanced/` naming war is resolved. PR-4c now targets `sqlite/pipeline/` with the correct eight-file layout — `mod.rs`, `retrieval.rs`, `rerank.rs`, `fusion.rs`, `scoring.rs`, `enrichment.rs`, `abstention.rs`, `decomp.rs` — matching `module-decomposition.md §2c` exactly. The scope table, the line-number assignments, and the directory name are now in agreement across all four specs. That was the most structurally dangerous misalignment in Round 1 and it has been corrected.

The `RetrievalStrategy` trait conflict is resolved. PR-4a has been split into PR-4a-i and PR-4a-ii. PR-4a-i's trait definition now explicitly aligns with `trait-surface.md §3.2` — `fn name() -> &str` and `async fn collect(ctx: &QueryContext) -> Result<CandidateSet>`. The design alignment note in PR-4a-i even names the spec section and explains why the unscored `CandidateSet` return preserves the fusion step. That is precise engineering documentation, and it is correct.

The missing `substrate/` clarity has been provided. The new "Relationship to trait-surface.md" section at the top of the roadmap explicitly states that this roadmap delivers 2 of the 7 substrate traits, names them, and declares that the full `substrate/` module is the v0.3.x campaign. `trait-surface.md` is named as the design reference for that future campaign. This resolves the "two parallel authorities" problem from Round 1.

The dependency graph is now correct. Phase 3 is no longer shown as a flat parallel block. The diagram explicitly shows `PR-3b (depends on PR-2d) ──> PR-3c` and `PR-3a (independent, parallel with PR-3b)`. The prose reinforces this. The Phase 2 serial chain is correctly diagrammed.

PR-2d now has a 10-sample gate requirement, consistent with Saruman's own concession in R1. The Quality Gate Summary table makes this explicit. The benchmark gate section now references `docs/benchmarks/benchmark_log.csv` and specifies that results must be appended with `bench.sh`, not raw `cargo run`.

The visibility cascade risk for PR-2c is now Risk #6 in the Risk Registry, with a specific mitigation: re-export `SqliteStorage` from `mod.rs` immediately as the first step, before moving any methods. The PR-2b chain-blocking risk is now Risk #7. The PR-1c scope expansion risk (ConnPool concurrent readers) is now Risk #8. The Risk Registry has grown from five entries to eight.

The branch naming protocol has been added. Every PR has a bookmark name. The jj rebase commands are spelled out for each dependency transition. The worktree isolation instruction is present.

The PR-3c conformance suite no longer panics. The scope note explicitly states that `AdvancedSearcher` and `PhraseSearcher` are excluded from the generic bound because `MemoryStorage` stubs them with `unimplemented\!()`. The suite covers the common-subset contract. The earlier "three options, choose one by accident" failure is resolved.

The PR-4a-ii `KeywordOnlyStrategy` now correctly returns `CandidateSet` per the trait contract, not pre-fused `Vec<SemanticResult>`. Scoring happens downstream in the pipeline. The dead-end abstraction is gone.

The PR-4c pre-split helper audit requirement is now explicit: "Audit shared helper types across `advanced.rs` and the target `pipeline/` files before splitting. If types like `RankedSemanticCandidate` or utility functions are shared across multiple target files, extract them to `mod.rs` re-exports or a `types.rs` first."

The `bench_strategy` binary ambiguity is resolved. PR-3a now consistently describes a `bench_strategy` binary; the "or a flag to `locomo_bench`" hedge is gone from the acceptance criterion, which reads `cargo run --release --bin bench_strategy -- --help`.

Thirteen issues raised. Thirteen addressed. The wizard is capable of listening, when the alternative is the Eye of Sauron bearing down on his documentation.

And yet.

---

### The Benchmark Gate Still Does Not Reference baselines.json

Saruman conceded this in his own defense: "The roadmap should reference `baselines.json` as the authoritative baseline source, not leave it implicit." He conceded it. He filed it as weakness W-5. And then he fixed everything except that.

The Quality Gate Summary table says:

> `./scripts/bench.sh --gate` (warns >2 pp, fails >5 pp)

Against what baseline? The bench.sh script currently uses a fragile CSV grep. The `benchmark-harness.md` spec defines `docs/benchmarks/baselines.json` as the explicit replacement. The roadmap Guiding Principles say "Benchmark-gated merges — any PR touching scoring, search, or storage passes `./scripts/bench.sh --gate`" with no mention of what the gate compares against.

An implementer who reads the roadmap and nothing else will run `bench.sh --gate` not knowing whether the gate reads from the old CSV or the new baselines.json. If `baselines.json` does not yet exist when PR-2b lands — and it will not, because no PR in the roadmap creates it — the gate will either fall back to the fragile CSV grep or fail with a KeyError. The `benchmark-harness.md` spec defines the file, its schema, and the replacement grep logic. The roadmap references that spec for PR-3a's scope, but does not state that the baselines.json gate must be in place before any benchmark-gated PR merges. That is a sequencing gap. PR-3a builds the strategy comparison harness. But `baselines.json` creation — and the `bench.sh` gate logic update to read from it — is not scoped to any PR.

This is not a catastrophic failure. It is a known gap that Saruman himself identified, declined to fix, and hoped would not be noticed in Round 2. It has been noticed.

---

### PR-3a's Scope Does Not Match the benchmark-harness.md Spec

The roadmap's PR-3a says: add a `bench_strategy` binary that runs the benchmark twice (once with `DefaultScoringStrategy`, once with an alternate strategy via CLI flag), outputs a side-by-side comparison table, and appends a row to `benchmark_log.csv` with a `strategy` column.

The `benchmark-harness.md` spec says something categorically different. The spec is not a standalone binary. It is a `--strategy` flag added to the existing `locomo_bench` binary, a `benches/locomo/strategies.rs` registry file, a `benches/bench_utils/stats.rs` P95 latency tracking module, two new fields on `LoCoMoSummary` (`strategy: String` and `p95_query_ms: f64`), and a `bench.sh --compare A B` mode that runs two strategy invocations and calls `compare_strategies.py`. The spec also defines `docs/benchmarks/baselines.json` as a new structured baseline file, replacing the CSV grep.

The roadmap's PR-3a and the benchmark-harness spec are describing different implementations of the same concept. The spec is richer, more precise, and was presumably written to govern PR-3a. But the roadmap does not reconcile with it. An implementer executing PR-3a will build a standalone `bench_strategy` binary. The spec says add `--strategy` to `locomo_bench`. These produce different binaries, different interfaces, and potentially conflicting `benchmark_log.csv` column schemas.

The `strategy` column in `benchmark_log.csv` — which the roadmap mentions PR-3a will add — conflicts with or duplicates the `strategy` field in `LoCoMoSummary` that the benchmark-harness spec adds to the existing struct. If the roadmap's standalone binary appends CSV rows directly, it bypasses the `LoCoMoSummary` serialization path that the spec intends. The data formats diverge.

This is a new misalignment introduced not by the reforging but by the original spec. However, the reforging did not fix it. The roadmap claims to be the single source of truth. When it conflicts with a companion spec on implementation details, it must resolve the conflict, not ignore it.

---

### The PR-4b Dependency Is Still Wrong

In Round 1 I noted that PR-4b has no logical dependency on Phase 3 work — `mcp_server.rs` splitting is independent of `MemoryStorage` or conformance suites — and that forcing it to wait after Phase 3 wastes calendar time. The PR Summary Table now shows PR-4b with "Depends On: —" (no dependency listed). Good. But the Phase 4 section header says: "Four PRs. PR-4a-i and PR-4b can be opened in parallel after Phase 3 merges."

The table says PR-4b has no dependency. The prose says PR-4b starts after Phase 3. These contradict each other. If PR-4b truly has no dependency, it can begin in parallel with Phase 2 or Phase 3, not forced to wait. The prose is the more restrictive statement and an implementer reading it will wait unnecessarily. The PR Summary Table — the authoritative reference — is correct. The prose is not. One of them needs to change.

---

### The AGENTS.md Update PR Is Still Missing

In Round 1 I noted: "After Phase 2-4 refactors, the Architecture section of AGENTS.md will list stale module paths (`src/mcp_server.rs`, `storage/sqlite/mod.rs`). No PR is responsible for keeping AGENTS.md current through the campaign."

PR-1b fixes the tool count in AGENTS.md. That is all PR-1b does. After Phase 4 lands, the Architecture section of AGENTS.md will reference `src/mcp_server.rs` (which will no longer exist — it becomes `src/mcp/`), `storage/sqlite/mod.rs` (which becomes a thin re-export facade), and the monolithic `advanced.rs` (which becomes `pipeline/`). No PR in the roadmap updates AGENTS.md after Phase 4. The milestone checkpoints do not include an AGENTS.md update. The Parked Items do not list it. This is a documentation debt that will be discovered when the first agent opens the repo after v0.2.2 and finds that the Architecture section describes a codebase that no longer exists.

---

### The v0.2.1 Checkpoint Is Missing PR-3a

Look at the v0.2.1 checkpoint:

```
- [ ] PR-3a merged: strategy comparison harness in `bench_strategy` binary
- [ ] PR-3b merged: `MemoryStorage` backend with 10+ unit tests
- [ ] PR-3c merged: conformance suite passes for both backends
```

This is correct in content. But the PR Summary Table shows PR-3a with no dependency and PR-3b depending on PR-2d. The Phase 3 prose correctly states PR-3a is independent and parallel with PR-3b. These are consistent.

However: the v0.2.1 checkpoint does not list the condition that PR-2d must have merged before v0.2.1 work can be considered complete. PR-3b depends on PR-2d. If PR-2d is delayed (say, by PR-2b's 10-sample benchmark run confirming a regression that must be fixed), PR-3b is blocked. The v0.2.1 checkpoint could be entered with only PR-3a merged, leaving PR-3b and PR-3c unstarted. The checkpoint as written does not make this ordering constraint visible. A milestone that says "Phase 3 complete" but silently requires Phase 2 to be complete first is a checkpoint that will be ticked prematurely.

This is a minor issue — the dependency is stated elsewhere in the document — but milestone checkpoints exist precisely so that a developer can evaluate them without consulting the dependency graph. The v0.2.1 checkpoint should include a note: "All v0.2.0 PRs must be merged before PR-3b can begin."

---

### PR-4b Still Has the Spike Problem

The risk mitigation for PR-4b reads: "Spike the module split on a throwaway branch first to confirm the proc macros are module-agnostic." The module-decomposition spec has already documented the solution: the thin-wrapper pattern is the answer, with a concrete code example. The spec does not say "spike first" — it says this is how it works.

If the spike has been done — if the module-decomposition spec's thin-wrapper pattern is the result of that spike — the roadmap should say "spike completed; thin-wrapper pattern confirmed" rather than "spike first." Leaving "spike first" in the risk mitigation of a production roadmap makes it unclear whether this is known or speculative. If the spike has not been done, the roadmap is implementing against an unverified assumption that is contradicted by the apparent confidence of the module-decomposition spec.

This was raised in Round 1. It remains unresolved in Round 2. The wizard heard the objection, wrote around it, and hoped the ambiguity would fade. The Lidless Eye does not blink.

---

### PR-1c: The Deduplication Test Is Still Absent

The acceptance criterion for PR-1c reads: "`prek run` passes; `./scripts/bench.sh --gate` shows no regression; `TODO(#121)` comment removed."

In Round 1 I noted that the parallel join changes the semantics of `seen_ids` accumulation — from serial cross-iteration accumulation to a single post-join pass — and that this is a behavioral change requiring a unit test for result deduplication with overlapping sub-query results. The scope of PR-1c now correctly describes: "Merge results after join: collect all sub-results, then apply the `seen_ids` dedup and score-max logic in a single pass."

The behavioral change is acknowledged. The test is not required. The acceptance criteria demand a benchmark gate (which tests aggregate quality) but not a unit test that verifies deduplication correctness when two sub-queries return the same memory ID with different scores. A benchmark gate cannot detect a deduplication bug that preserves aggregate F1 while producing the wrong result for a specific case. The acceptance criterion is insufficient for a behavioral change of this nature.

---

### PR-1a: Full-Mode Regression Still Not Tested

Round 1 noted: "Acceptance criterion only verifies Minimal mode returns 4 tools. No test verifies Full mode still returns all 19."

The PR-1a acceptance criterion now reads: "`prek run` passes; `cargo test --all-features` includes the new test; comment is gone."

The "new test" asserts the filtered set contains exactly 4 facade names. This is unchanged from Round 1. Full mode — 19 tools — is still not tested in the acceptance criteria. This is a minor issue, but it was raised in Round 1 and was not addressed. I note it for the record.

---

### Summary of the Wizard's Performance

Thirteen issues from Round 1. Eleven addressed correctly. Two partially addressed (PR-1a Full-mode test, PR-4b spike ambiguity). One explicitly conceded but not fixed (baselines.json gate reference). One introduced by the reforging itself (PR-3a versus benchmark-harness.md scope divergence). Three new issues not in Round 1 (PR-4b dependency prose conflict, missing AGENTS.md update PR for post-Phase-4 architecture, v0.2.1 checkpoint missing Phase 2 prerequisite visibility).

The roadmap is substantially better than Round 1. A craftsman who repaired eleven of thirteen faults in a single iteration has demonstrated competence. What remains — the baselines.json gap, the PR-3a/benchmark-harness misalignment, the prose/table contradiction on PR-4b — is the difference between a competent refactor plan and a plan that will not cause an implementation agent to stop, ask questions, and waste the wizard's time.

The Palantir has spoken again. Whether Saruman listens again is, as always, a matter of character.

---

## FINDINGS

### R1 Issues — Verification Status

- [verified-fixed] [R1-critical-1] admin.rs missing PR: PR-1d now exists with full scope, ordered execution, file size exceptions, and correct acceptance criteria.
- [verified-fixed] [R1-critical-2] pipeline/ vs advanced/ naming mismatch: PR-4c now targets `sqlite/pipeline/` with 8-file layout matching module-decomposition.md §2c exactly.
- [verified-fixed] [R1-critical-3] RetrievalStrategy incompatible trait signatures: PR-4a split into 4a-i/4a-ii; 4a-i explicitly aligns with trait-surface.md §3.2 signature (name/collect/CandidateSet).
- [verified-fixed] [R1-important-1] substrate/module relationship not acknowledged: "Relationship to trait-surface.md" section added at top of roadmap; v0.3.x campaign clearly delineated.
- [verified-fixed] [R1-important-2] bench_strategy vs locomo_bench ambiguity in PR-3a acceptance: "or a flag" hedge removed; acceptance criterion now reads `cargo run --release --bin bench_strategy`.
- [verified-fixed] [R1-important-3] PR-4c acceptance criterion requires <500 lines but spec grants admin/ exceptions: acceptance criterion now explicitly cites the module-decomposition.md §4 exceptions.
- [verified-fixed] [R1-important-4] Phase 3 flat dependency diagram wrong: diagram and prose now correctly show PR-3a parallel with PR-3b, and PR-3c serial after PR-3b.
- [verified-fixed] [R1-important-5] PR-2d missing 10-sample gate: Quality Gate Summary now explicitly lists PR-2d as requiring 10-sample manual gate.
- [verified-fixed] [R1-important-6] Visibility cascade risk unregistered: Risk #6 in registry with specific re-export-first mitigation.
- [verified-fixed] [R1-important-7] PR-2b benchmark warning chain-blocking unregistered: Risk #7 in registry.
- [verified-fixed] [R1-important-8] PR-1c ConnPool scope expansion unregistered: Risk #8 in registry.
- [verified-fixed] [R1-important-9] PR-3b/3c conformance suite trait-bound panic: PR-3c scope note explicitly excludes AdvancedSearcher/PhraseSearcher from generic bound.
- [verified-fixed] [R1-important-10] Branch naming and jj rebase protocol absent: full Branch Naming and Rebase Protocol section added with bookmark table and rebase commands.
- [partially-fixed] [R1-important-11] PR-4b spike ambiguity (module-decomp says thin-wrapper is confirmed; roadmap says spike first): spike language retained without clarifying if it is resolved. See new finding F-4.
- [verified-fixed] [R1-important-12] PR-2c benchmark gate suppressed vs module-decomp §6 requirement: PR-2c acceptance criterion now references module-decomposition.md §6 test gates and includes `./scripts/bench.sh --gate`.
- [partially-fixed] [R1-minor-1] PR-1a Full-mode regression not tested: unchanged. See new finding F-6.
- [not-fixed] [R1-minor-5] Benchmark gate does not reference baselines.json: Saruman conceded this (W-5) but did not fix it. See new finding F-1.
- [verified-fixed] [R1-important-PR-4a] KeywordOnlyStrategy dead-end abstraction: PR-4a-ii now correctly returns CandidateSet; scoring happens downstream.
- [verified-fixed] [R1-important-PR-4c] Shared helper audit missing from PR-4c: pre-split audit requirement now explicit in PR-4c.

---

### New Findings (Round 2)

- [severity:important] [execution-roadmap.md Quality Gate Summary / benchmark-harness.md §4] The benchmark gate command `./scripts/bench.sh --gate` is listed in the Quality Gate Summary with no reference to `baselines.json` as the authoritative baseline source. `benchmark-harness.md §4` defines `docs/benchmarks/baselines.json` as a structured replacement for the fragile CSV grep. No PR in the roadmap creates `baselines.json` or updates `bench.sh` gate logic to read from it. If `baselines.json` does not exist when any benchmark-gated PR lands, the gate falls back to CSV grep or fails. Fix: add a note to the Quality Gate Summary that the gate reads from `docs/benchmarks/baselines.json`; scope the creation of that file and the bench.sh gate-logic update (per benchmark-harness.md §4) to PR-3a or a new PR-0.

- [severity:important] [execution-roadmap.md PR-3a / benchmark-harness.md §1-§5] PR-3a's scope and the benchmark-harness.md spec describe incompatible implementations of the strategy comparison feature. Roadmap: standalone `bench_strategy` binary that runs the benchmark twice and appends a `strategy`-column CSV row. Benchmark-harness spec: `--strategy` flag on `locomo_bench`, `benches/locomo/strategies.rs` registry, `benches/bench_utils/stats.rs` P95 latency module, `LoCoMoSummary` struct extension (`strategy`, `p95_query_ms` fields), `bench.sh --compare A B` mode, `compare_strategies.py` script, and `baselines.json`. These produce different binaries, different CLI interfaces, different CSV schemas, and different output artefacts. The "single source of truth" claim fails here. Fix: reconcile PR-3a's scope with benchmark-harness.md; either the roadmap governs and the benchmark-harness spec must be updated, or the benchmark-harness spec governs and PR-3a's scope must be expanded significantly.

- [severity:important] [execution-roadmap.md Phase 4 prose vs PR Summary Table] The Phase 4 section header states "PR-4a-i and PR-4b can be opened in parallel after Phase 3 merges," implying PR-4b waits for Phase 3. The PR Summary Table shows PR-4b with "Depends On: —" (no dependency). These contradict each other. The table is authoritative and correct — PR-4b has no logical dependency on Phase 3. The prose must match: either remove the "after Phase 3 merges" qualifier for PR-4b, or make the table reflect a Phase 3 dependency with a stated reason. Fix: align prose to match the dependency table; PR-4b can begin in parallel with Phase 2 or Phase 3.

- [severity:important] [execution-roadmap.md PR-4b Risk Mitigations] The rmcp spike ambiguity persists. The risk mitigation states "Spike the module split on a throwaway branch first to confirm the proc macros are module-agnostic." The module-decomposition spec documents the thin-wrapper pattern as confirmed architecture, including a working code example, with no "spike first" qualifier. If the spike is done, the roadmap should say so. If it is not done, then PR-4b's module-decomposition spec is speculative documentation, not a confirmed execution guide, and the risk should be rated High not Medium. The current state is a contradiction between a risk mitigation that implies uncertainty and a companion spec that implies certainty. Fix: either mark the spike as completed (citing the module-decomposition spec as its output), or re-rate PR-4b risk to High and acknowledge the spike as an open prerequisite.

- [severity:important] [execution-roadmap.md §Milestone Checkpoints, v0.2.1] The v0.2.1 checkpoint lists three items (PR-3a, PR-3b, PR-3c merged). PR-3b depends on PR-2d, which is Phase 2 work. The v0.2.1 checkpoint does not state that Phase 2 must be fully complete before PR-3b can begin. A developer evaluating v0.2.1 completion status could observe PR-3a merged and attempt to open PR-3b while PR-2d is still in flight, causing a compilation failure (missing `ScoringStrategy` injection). Fix: add a gate condition to the v0.2.1 checkpoint: "Prerequisite: v0.2.0 checkpoint complete (all Phase 2 PRs merged) before PR-3b begins."

- [severity:minor] [execution-roadmap.md PR-1a] Acceptance criterion still does not require verifying Full mode (all 19 tools) after implementing Minimal mode filtering. A bug in the `list_tools` override that incorrectly filters Full mode would not be caught by the specified test. Fix: add "unit test asserting that non-Minimal mode still returns all 19 tool names" to the acceptance criteria.

- [severity:minor] [execution-roadmap.md PR-1c Acceptance] Acceptance criterion does not require a unit test for sub-query deduplication correctness with overlapping results. The scope correctly describes the behavioral change (seen_ids from serial accumulation to post-join single pass). A benchmark gate measures aggregate quality, not per-ID deduplication correctness. A query decomposed into two sub-queries that both retrieve the same memory ID should produce exactly one result with the higher score; this is not tested by the benchmark. Fix: add acceptance criterion: "unit test asserting that overlapping sub-query results (same memory_id, different scores) produce exactly one result with score = max(scores)."

- [severity:minor] [execution-roadmap.md §Parked Items / AGENTS.md] No PR updates AGENTS.md after Phase 4's structural changes. After Phase 4 merges, AGENTS.md Architecture section will reference `src/mcp_server.rs` (replaced by `src/mcp/`), `storage/sqlite/mod.rs` as a monolithic file (now a thin re-export facade), and the monolithic `advanced.rs` (now `pipeline/`). PR-1b fixes the tool count but not the module paths. Fix: add AGENTS.md architectural documentation update to PR-4b's acceptance criteria (when mcp_server.rs is split) and PR-4c's acceptance criteria (when advanced.rs becomes pipeline/).

