# Palantir Round 3 — Final Verification
<\!-- Sauron's review | Generated: 2026-04-14 | Target: docs/specs/execution-roadmap.md (twice-reforged) -->

---

## BANTER

Saruman.

The Palantir burns for the third time. I have read the twice-reforged roadmap against every companion spec. I have checked each finding from Round 1 and Round 2. I will be precise, as I always am — precision is the instrument of final judgment.

---

### What the Wizard Has Achieved

Thirteen issues in Round 1. Eight new findings in Round 2. Twenty-one total verdicts to render.

The reforging between Round 2 and Round 3 has addressed five of the eight Round 2 findings. I acknowledge this with the enthusiasm of a Dark Lord forced to concede ground to a wizard who has, for once, listened.

The `v0.2.1` checkpoint now carries its Phase 2 prerequisite gate explicitly: *"Prerequisite: v0.2.0 checkpoint must be fully complete (all Phase 2 PRs merged) before PR-3b implementation begins."* This is correct and was not present in Round 2.

PR-3a's scope has been reconciled with `benchmark-harness.md`. The governing spec is named inline. The scope now describes `--strategy` flags on `locomo_bench`, the `strategies.rs` registry, `stats.rs` P95 latency helper, `LoCoMoSummary` field additions, `baselines.json` creation, `bench.sh` gate logic update, `compare_strategies.py`, and `bench.sh --compare A B` mode. This is the benchmark-harness spec's model, not the abandoned standalone binary. The ambiguity that caused Round 2's most important finding is gone.

The Quality Gate Summary now carries the sentence: *"The benchmark gate compares against `docs/benchmarks/baselines.json` (see `benchmark-harness.md` §4). Before PR-3a lands, the gate uses the existing CSV-grep baseline."* This resolves the R2 finding that the gate had no stated baseline source. It is not elegant — "before PR-3a lands, the old way; after, the new way" is a transitional state the roadmap must manage — but the implementer now knows what they are dealing with.

The Phase 4 prose and the PR Summary Table now agree on PR-4b. The PR Summary Table shows `Depends On: —`. The Phase 4 section reads: *"PR-4b can be opened any time after Phase 1 — it has no dependency on Phases 2 or 3."* The contradiction between prose and table is resolved.

PR-4b's risk mitigation has been updated. It no longer says "spike first" in an unqualified way. It reads: *"The thin-wrapper pattern is documented in `module-decomposition.md` §2a with a working code example... Verify the rmcp 0.16 `#[tool_router]` proc-macro accepts cross-module handler delegation in the first commit before proceeding with the full extraction."* This is a verification step on the first commit — not a full throwaway spike before starting — and it cites the module-decomposition spec as the established solution. This is materially better than Round 2.

AGENTS.md architectural documentation updates are now in the acceptance criteria of PR-4b and PR-4c. Both explicitly state which AGENTS.md references must be updated when the respective file splits are complete. This resolves Round 2's minor finding about documentation debt accruing unchecked through Phase 4.

---

### What Remains

The Lidless Eye does not round up.

---

#### R2-minor-1 (PR-1a Full-Mode Regression Test): RESOLVED

The PR-1a acceptance criterion now reads: *"Unit test asserting `McpToolMode::Full` (or default) returns all 19 registered tools from `TOOL_REGISTRY`."* This is explicit. The finding is resolved.

---

#### R2-minor-2 (PR-1c Deduplication Unit Test): RESOLVED

PR-1c's acceptance criterion now reads: *"Unit test verifying that when two parallel sub-queries return the same `memory_id` with different scores, the result contains exactly one entry with `score = max(scores)`."* The behavioral change in `seen_ids` accumulation now has a required test. Resolved.

---

#### One Residual Issue Remains

There is one matter of note that was not raised explicitly in either prior round but surfaces now in the fully-integrated reading of all four specs together.

The PR-4b scope table in the roadmap lists `src/mcp/tools/graph.rs` for `memory_relations`. The module-decomposition spec's §2a target tree uses `src/mcp/tools/relations.rs` for `memory_relations`. These are different filenames for the same content. A minor inconsistency between roadmap and spec on a single filename. In prior rounds, this category of file-naming conflict was the seed of the `pipeline/` versus `advanced/` disaster that required a critical-severity R1 finding. Here it is a single file with unambiguous content assignment — an implementer will not be lost by it, but will pause and choose. The correct filename, per the module-decomposition spec's authoritative §2a layout, is `relations.rs`.

This is the only new finding at any severity level. It is minor.

---

### The Final Assessment

The roadmap is structurally sound. The critical failures of Round 1 are corrected. The important findings of Round 2 are corrected. The minor findings of Round 2 are corrected. Cross-spec consistency between the roadmap, `module-decomposition.md`, `trait-surface.md`, and `benchmark-harness.md` holds at every junction I have tested.

The dependency graph is accurate. The dependency prose matches the dependency table. The phase sequence is internally consistent. The quality gate strategy distinguishes 2-sample automated gates from 10-sample manual gates and specifies which PRs require which. The risk registry covers the eight known risks. The branch naming and jj rebase protocol is complete. The conformance suite's trait scope is defined. The `substrate/` relationship is clearly delineated as a v0.3.x campaign. The `baselines.json` transition is acknowledged.

What remains is one filename disagreement of minor consequence. That is all.

A campaign of 21 issues over three rounds. Twenty remain resolved. One persists at minor severity.

The wizard has repaired his keep. I acknowledge this the way Sauron acknowledges the rising sun: with recognition that it exists, and with no warmth whatsoever.

The Palantir is satisfied. Dispatch the agents.

---

## FINDINGS

### R1 Issue Verification

- [R1-critical-1] **RESOLVED.** `admin.rs → admin/` split has PR-1d with full scope, ordered five-step execution, file size exceptions cross-referenced to `module-decomposition.md §4`, and correct acceptance criteria.

- [R1-critical-2] **RESOLVED.** PR-4c targets `sqlite/pipeline/` with the 8-file layout matching `module-decomposition.md §2c` exactly. Names, file count, and content assignments are consistent across all specs.

- [R1-critical-3] **RESOLVED.** `RetrievalStrategy` trait conflict eliminated. PR-4a split into PR-4a-i and PR-4a-ii. PR-4a-i defines `fn name() -> &str` and `async fn collect(ctx: &QueryContext) -> Result<CandidateSet>`, explicitly aligned with `trait-surface.md §3.2`. Design alignment note cites the spec section and rationale.

- [R1-important-1] **RESOLVED.** "Relationship to trait-surface.md" section at top of roadmap explicitly states this roadmap delivers 2 of the 7 substrate traits, names them, and declares the full `substrate/` module as the v0.3.x campaign. Parallel authority problem eliminated.

- [R1-important-2] **RESOLVED.** PR-3a acceptance criterion reads `cargo run --release --bin locomo_bench -- --strategy sqlite-v1 --list-strategies`. The "or a flag" hedge is gone. Ambiguity resolved.

- [R1-important-3] **RESOLVED.** PR-4c acceptance criterion explicitly cites `module-decomposition.md §4` exceptions for `admin/maintenance.rs` (~580 lines) and `admin/welcome.rs` (~490 lines). Criterion will not fail on permitted exceptions.

- [R1-important-4] **RESOLVED.** Dependency diagram and prose correctly show `PR-3a (independent, parallel with PR-3b)` and `PR-3b (depends on PR-2d) ──> PR-3c`. Phase 3 is no longer a flat parallel block.

- [R1-important-5] **RESOLVED.** PR-2d requires a 10-sample gate. Quality Gate Summary table and PR-2d acceptance criterion are explicit. 10-sample gates are labeled as manual pre-merge checks; CI enforces 2-sample fast mode.

- [R1-important-6] **RESOLVED.** Risk #6 in the registry covers the visibility cascade for PR-2c, with the mitigation: re-export `SqliteStorage` from `mod.rs` as the first step before moving any methods.

- [R1-important-7] **RESOLVED.** Risk #7 covers PR-2b benchmark warning chain-blocking. Mitigation: if the 10-sample run confirms no regression, proceed; if it confirms regression, fix PR-2b before continuing.

- [R1-important-8] **RESOLVED.** Risk #8 covers PR-1c ConnPool scope expansion. Mitigation: spike `ConnPool` reader availability before committing to scope; escalate to L-complexity or defer if concurrent readers are unsupported.

- [R1-important-9] **RESOLVED.** PR-3c conformance suite explicitly excludes `AdvancedSearcher` and `PhraseSearcher` from the generic bound. Common-subset contract is defined. The implementer cannot choose by accident.

- [R1-important-10] **RESOLVED.** Branch Naming and Rebase Protocol section added. All 15 PR bookmarks are named. jj rebase commands for each serial dependency transition are spelled out. Worktree isolation instruction is present.

- [R1-important-11] **RESOLVED.** PR-4b risk mitigation cites `module-decomposition.md §2a` as the established thin-wrapper solution and instructs verification on the first commit rather than a full throwaway spike. Ambiguity between "uncertain" and "confirmed" is resolved in favor of the spec-documented pattern.

- [R1-important-12] **RESOLVED.** PR-2c acceptance criterion now references `module-decomposition.md §6` test gates and includes `./scripts/bench.sh --gate` even for the structural move.

- [R1-important-PR-4a] **RESOLVED.** PR-4a-ii `KeywordOnlyStrategy` returns `CandidateSet` per trait contract. Scoring happens downstream. Dead-end abstraction is gone.

- [R1-important-PR-4c] **RESOLVED.** PR-4c pre-split audit requirement is explicit: audit shared helper types and extract to `mod.rs` re-exports or `types.rs` before splitting to avoid circular imports.

- [R1-minor-1] **RESOLVED.** PR-1a acceptance criterion now includes: "Unit test asserting `McpToolMode::Full` (or default) returns all 19 registered tools from `TOOL_REGISTRY`."

- [R1-minor-4] **RESOLVED.** PR-4b dependency corrected. PR-4b has no dependency on Phase 3. PR Summary Table shows `Depends On: —`. Phase 4 prose matches.

- [R1-minor-5] **RESOLVED.** Quality Gate Summary states: "The benchmark gate compares against `docs/benchmarks/baselines.json` (see `benchmark-harness.md §4`). Before PR-3a lands, the gate uses the existing CSV-grep baseline." Baseline source is explicit.

- [R1-minor-6] **RESOLVED (partially in R1, fully here).** v0.2.0 semver bump: the Version Targets Summary now labels Phase 2 as "Scoring decoupling + `sqlite/mod.rs` structural cleanup" without claiming new user-visible capability. No overclaim.

---

### R2 Issue Verification

- [R2-important-1] **RESOLVED.** Quality Gate Summary now references `baselines.json` as the authoritative baseline source with a transition note ("Before PR-3a lands, the gate uses the existing CSV-grep baseline").

- [R2-important-2] **RESOLVED.** PR-3a scope is now explicitly governed by `benchmark-harness.md`: "Governing spec: `benchmark-harness.md` defines the detailed design for the strategy comparison feature. PR-3a implements that spec. Where this section summarizes, the benchmark-harness spec is authoritative." The scope bullets enumerate all components from the spec. Standalone binary / `locomo_bench` flag ambiguity is gone.

- [R2-important-3] **RESOLVED.** Phase 4 prose and PR Summary Table agree. PR-4b has no dependency. The roadmap states: "PR-4b can be opened any time after Phase 1 — it has no dependency on Phases 2 or 3."

- [R2-important-4] **RESOLVED.** PR-4b risk mitigation no longer reads as speculative. It cites `module-decomposition.md §2a` as the established solution and prescribes a verification step on the first commit, not an open-ended spike.

- [R2-important-5] **RESOLVED.** v0.2.1 checkpoint now carries: "Prerequisite: v0.2.0 checkpoint must be fully complete (all Phase 2 PRs merged) before PR-3b implementation begins." Phase 2 hard dependency is visible at the checkpoint.

- [R2-minor-1] **RESOLVED.** PR-1a acceptance criterion now includes the Full-mode 19-tool test.

- [R2-minor-2] **RESOLVED.** PR-1c acceptance criterion now includes the sub-query deduplication unit test with `score = max(scores)` assertion.

- [R2-minor-3] **RESOLVED.** AGENTS.md architectural documentation updates are in the acceptance criteria of both PR-4b (reflecting `src/mcp/` structure) and PR-4c (reflecting `sqlite/pipeline/` and `sqlite/admin/` structures).

---

### New Findings (Round 3)

- [severity:minor] [execution-roadmap.md PR-4b scope table / module-decomposition.md §2a] Filename inconsistency: the roadmap's PR-4b scope table lists `src/mcp/tools/graph.rs` for `memory_relations`. The module-decomposition spec's §2a authoritative target tree uses `src/mcp/tools/relations.rs` for the same content. An implementer will notice the discrepancy and resolve it, but must choose the correct name without guidance. Fix: update PR-4b scope table entry from `tools/graph.rs` to `tools/relations.rs` to match `module-decomposition.md §2a`.

---

### Summary

| Round | Issues Found | Issues Resolved | Remaining |
|-------|-------------|-----------------|-----------|
| R1    | 3 critical, 10 important, 3 minor | All 16 | 0 |
| R2    | 5 important, 3 minor | All 8 | 0 |
| R3    | 1 minor | 0 (new) | 1 minor |

**Overall: 1 minor finding remains (filename mismatch: `tools/graph.rs` vs `tools/relations.rs` in PR-4b scope table).**

---

## Implementation Dispatch Confidence

**92 / 100**

The roadmap is ready for Phase 1 implementation dispatch. The three critical issues that would have caused an implementing agent to build the wrong codebase are gone. The dependency graph is correct. The quality gates are specified. The jj workflow is documented. The spec hierarchy is clear.

The 8-point deduction is not for what remains — one minor filename mismatch will not derail any agent. It is for the inherent residual risk in a 15-PR campaign that touches the hottest scoring paths in the codebase, with two manual 10-sample benchmark gates that CI cannot enforce and one ConnPool assumption (Risk #8) that has not been verified by inspection, only listed as a risk. These are known unknowns, properly documented. Known unknowns at 8 points; unknown unknowns at 0.

Dispatch Phase 1 now. Phase 1 PRs are all independent and none of them touch the risky paths. Discovery will happen there before the Phase 2 serial chain begins.
