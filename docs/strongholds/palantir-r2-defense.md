# Palantir Defense — Round 2
<\!-- Saruman the White, Lord of Isengard, defends the REFORGED MAG Execution Roadmap -->
<\!-- Generated: 2026-04-14 | Roadmap baseline: v0.1.9-dev | Horizon: v0.2.2 | Round: 2 -->

---

## BANTER

*The Palantir blazes white again. Saruman sets down his staff very deliberately, smooths his robes, and allows himself the smallest, most controlled smile.*

Round 2. The Dark Lord returns. And I have been... *prepared*.

Let me tell you what has happened since Round 1, Sauron, because I suspect you gazed at the reforged document expecting to find the same wasteland of contradictions — and instead found something considerably more... formidable.

**On the six concessions from Round 1 — yes, let us account for them first.**

I conceded six issues. SIX. Let us see how many survive your second assault.

**W-1: No paragraph relating roadmap to trait-surface.md.** I said this was High severity. I said add a paragraph. The reforged roadmap now has an *entire section* — "Relationship to trait-surface.md" — that is frankly more thorough than the "paragraph" I proposed. It names which 2-of-7 traits are implemented, names the 5 remaining traits by name, declares v0.3.x as the substrate campaign, establishes authority hierarchy ("this roadmap governs for Phases 1-4; for work beyond v0.2.2, trait-surface.md governs"), and explicitly says PR-4a-i is aligned with trait-surface.md §3.2. That is not a patch. That is a proper fix. W-1: **RESOLVED**.

**W-2: PR-2d missing 10-sample benchmark gate.** I conceded this. The reforged PR-2d acceptance criterion now reads — and I will quote it directly — "`./scripts/bench.sh --samples 10 --notes "PR-2d pre-merge"` shows no regression (10-sample gate required)." AND the Quality Gate Summary table now shows PR-2d explicitly in the "Benchmark 10-sample" row alongside PR-4a-ii. AND the Quality Gate Summary adds the note: "10-sample gates are manual pre-merge checks. CI enforces 2-sample fast gates automatically. The 10-sample requirement for PR-2d and PR-4a-ii must be verified by the developer before merge." W-2: **RESOLVED**, and more thoroughly than requested.

**W-3: PR-4a should be split into 4a-i and 4a-ii.** I proposed this split myself in Round 1. The reforged roadmap has implemented it exactly as I specified: PR-4a-i is additive (Risk: Low, no benchmark gate), PR-4a-ii is behavioral (Risk: High, 10-sample gate). The trait definition in PR-4a-i is now explicitly aligned with trait-surface.md §3.2 — `fn name() -> &str` and `async fn collect(ctx: &QueryContext) -> CandidateSet`. The RetrievalStrategy incompatibility that Sauron attacked so vigorously in Round 1 is *gone*. Both the roadmap and the trait-surface spec now agree. W-3: **RESOLVED**, and the incompatible trait signature is corrected simultaneously.

**W-4: Shared helper audit before advanced.rs split.** I conceded this as Low severity. The reforged PR-4c now has an explicit "Pre-split requirement" paragraph: "Audit shared helper types across advanced.rs and the target pipeline/ files before splitting. If types like RankedSemanticCandidate or utility functions are shared across multiple target files, extract them to mod.rs re-exports or a types.rs first. This avoids circular imports during the split." W-4: **RESOLVED**.

**W-5: Benchmark gate does not reference baselines.json.** I conceded this. The Quality Gate Summary now says: "Results must be appended to `docs/benchmarks/benchmark_log.csv` using `bench.sh` (not raw `cargo run`)." It does not yet explicitly name baselines.json as the gate source — that is still implied. This is a *minor* residual. I will note it honestly in the findings. **PARTIALLY resolved**.

**And the most devastating attack from Round 1 — the admin.rs black hole.** Sauron declared, correctly and devastatingly, that admin.rs had *no PR*. 1,619 lines of four independent trait groups, falling through the floor. I could not defend that. I did not defend it. And behold: the reforged roadmap now has **PR-1d**, a full Phase 1 PR with a complete scope table, 5-step execution order, file size exceptions pre-justified, and the explicit statement that it uses module-decomposition.md §2d as its reference. The admin.rs problem is not merely patched — it is *correctly placed in Phase 1*, exactly where the module-decomposition spec always said it belonged. **RESOLVED**.

*Saruman pauses. Six concessions verified. The score stands. Now let us see what the Dark Lord brought for Round 2.*

**On the dependency graph corrections — let me examine what was fixed.**

Sauron attacked the Phase 3 dependency diagram in Round 1, correctly. The reforged diagram now reads:
```
PR-3a (independent, parallel with PR-3b)
PR-3b (depends on PR-2d) ──> PR-3c
```

That is *correct*. PR-3b's dependency on PR-2d is explicit. PR-3c's dependency on PR-3b is explicit. The "three parallel PRs" fiction is gone. The dependency is clearly stated in the preamble: "PR-3a is independent. PR-3b depends on PR-2d... PR-3c depends on PR-3b." W-3c: **RESOLVED**.

**On the conformance suite vs. MemoryStorage unimplemented\!() tension.** Sauron found a genuine ambiguity in Round 1. The reforged PR-3c now has an explicit "Trait scope note" paragraph: "The conformance suite tests the trait subset that MemoryStorage implements... It does NOT include AdvancedSearcher or PhraseSearcher in the generic bound — MemoryStorage stubs these with unimplemented\!()... so including them would panic at test time. The suite covers the common-subset contract; SQLite-specific traits are tested by the existing tests.rs suite." The implementer will NOT choose by accident. They have explicit guidance. **RESOLVED**.

**On the pipeline/ vs advanced/ layout conflict.** The reforged PR-4c now *correctly* targets `sqlite/pipeline/` with all 8 files matching the module-decomposition spec: `mod.rs`, `retrieval.rs`, `rerank.rs`, `fusion.rs`, `scoring.rs`, `enrichment.rs`, `abstention.rs`, `decomp.rs`. The "advanced/" fiction is gone. **RESOLVED**.

**On the bench_strategy "or" ambiguity.** The reforged PR-3a still says "a `bench_strategy` binary (or a flag to `locomo_bench`)" — but this is now a weaker criticism than it was in Round 1, because the benchmark-harness spec is referenced for the implementation details. Still, I will be honest: the "or" remains. The benchmark-harness spec clearly defines this as a `--strategy` flag on `locomo_bench`, not a separate binary. The roadmap's acceptance criterion still uses `cargo run --release --bin bench_strategy`. That is a *residual inconsistency* with the spec. Minor, but present.

**On the PR-4c acceptance criterion vs. the admin/ exceptions.** The reforged PR-4c acceptance criterion now reads: "file sizes are all under 500 lines (with `admin/maintenance.rs` ~580 and `admin/welcome.rs` ~490 exceptions per `module-decomposition.md` §4)." The contradiction from Round 1 is **RESOLVED**. Explicitly.

**On the 10-sample CI enforcement.** Sauron found that the CI job only runs 2 samples, so PR-4a-ii's "10-sample gate" would be unenforceable in CI. The reforged Quality Gate Summary now explicitly says: "10-sample gates are manual pre-merge checks. CI enforces 2-sample fast gates automatically." The implementer will not be confused. **RESOLVED**.

**On the risk registry gaps.** The reforged risk registry now has 8 items (up from 5). Risks 6, 7, and 8 are new additions:
- Risk 6: Visibility cascade during PR-2c. Present and well-mitigated (re-export as first step).
- Risk 7: PR-2b warning blocks PR-2c/PR-2d chain. Present and well-articulated.
- Risk 8: PR-1c scope expansion if ConnPool lacks concurrent readers. Present.

These were three of Sauron's Round 1 findings. All three are now registered. **RESOLVED**.

**On the branch naming and jj rebase protocol.** A new "Branch Naming and Rebase Protocol" section exists, complete with a table of 15 bookmark names and explicit `jj rebase -b` commands for the serial chain. **RESOLVED**.

**Now — and this is where Saruman takes the offensive — let me identify what REMAINS.**

*The staff strikes the floor once. The Palantir flares.*

**First residual: bench_strategy binary vs. --strategy flag.** PR-3a's acceptance criterion says `cargo run --release --bin bench_strategy -- --help`. The benchmark-harness spec defines the strategy comparison as a `--strategy` flag on the existing `locomo_bench` binary. These are still inconsistent. If an implementer follows the roadmap's acceptance criterion, they will create a separate binary. If they follow the benchmark-harness spec, they will add a flag. This is not a critical blocker — but it is a genuine ambiguity that an implementer will need to resolve, and the resolution will not be obvious.

**Second residual: the PR-1a acceptance criterion still only checks Minimal mode.** Sauron found this in Round 1. The reforged PR-1a acceptance says: "Add a unit test asserting the filtered set contains exactly the 4 facade names." It does NOT require a test that Full mode still returns all 19. A regression in Full mode is still undetectable by the specified acceptance criterion. This is minor, but it is the same gap as Round 1.

**Third residual: baselines.json explicit reference.** The Quality Gate Summary says results go to benchmark_log.csv. It does not explicitly name baselines.json as the gate comparison source. This matters because bench.sh currently uses fragile CSV-grep for the gate, and the benchmark-harness spec explicitly replaces it with baselines.json. The roadmap should name baselines.json as the gate source, not leave it implied.

**Fourth — and this I find only now, on close examination — PR-4b can begin earlier.** The reforged Phase 4 dependency diagram shows PR-4b starting "after Phase 3 merges." But PR-4b (mcp_server.rs split) has no logical dependency on Phase 3 work. MemoryStorage and the conformance suite are entirely orthogonal to the MCP server structure. PR-4b could begin in parallel with Phase 2 if calendar time is important. The PR Summary Table shows PR-4b with no dependency ("—"), which contradicts the Phase 4 preamble that implies it starts after Phase 3. That inconsistency within the roadmap itself is new and introduced by the reforging.

**Wait.** Let me look at this precisely. The Phase 4 preamble says: "Four PRs. PR-4a-i and PR-4b can be opened in parallel *after Phase 3 merges*." But the PR Summary Table lists PR-4b as depending on "—". If PR-4b has no dependencies, it can start at any time — not just after Phase 3. This is a minor internal inconsistency, but it is one the reforge *introduced* rather than inherited. The original roadmap at least had PR-4b waiting on Phase 3 *explicitly* (even if that was wrong). The reforge made the dependency table say "—" but left the prose saying "after Phase 3."

**Fifth, and most interesting: the Quality Gate Summary still uses a uniform 2pp/5pp threshold for structural refactor PRs.** Sauron attacked this correctly. For PR-2c, PR-4b (no gate), PR-4c: pure code moves should have a tighter gate. The reforged Quality Gate Summary still shows:
```
Benchmark 2-sample | PRs touching scoring/search/storage | ./scripts/bench.sh --gate (warns >2 pp, fails >5 pp)
```
The differentiation between structural-move PRs and algorithmic PRs is not reflected in the gate thresholds. I defended the current threshold in Round 1 on the grounds that 5pp is sufficient to catch catastrophic regressions, and 0pp gating implies false precision. I will hold that defense. But I acknowledge this remains a legitimate point of disagreement.

*The Palantir dims slightly. Saruman straightens.*

The verdict on the reforged roadmap: this is, by any honest measure, a substantially better document. Twelve of Sauron's thirteen Round 1 findings have been addressed. Five of my six concessions are fully resolved. One is partially resolved. Three new minor issues have been introduced or remain. The critical issues — admin.rs having no PR, the RetrievalStrategy incompatibility, the pipeline/ vs. advanced/ layout conflict — are ALL gone.

Is this roadmap ready for Phase 1 implementation? **Yes, for Phase 1.** PR-1a, PR-1b, PR-1d are clean. PR-1c needs the ConnPool spike first (now explicitly documented in Risk 8). Phase 2 is ready once Phase 1 validates execution process. The four remaining gaps are minor and addressable in-flight by the implementing agent.

**Confidence level: 85%.** The remaining 15% is: the bench_strategy binary ambiguity (needs one sentence to resolve), the PR-4b scheduling inconsistency (needs one word edit), the PR-1a Full-mode test gap (needs one test added), and the baselines.json reference (needs one sentence). None of these block implementation. All can be resolved in PR review.

*Saruman the White. Saruman the Wise. Still standing.*

---

## FINDINGS

### R1 Concession Resolution Status

- [resolved] [execution-roadmap.md: "Relationship to trait-surface.md" section] W-1 FIXED. New section explicitly names 2-of-7 traits delivered, names all 5 remaining traits, establishes v0.3.x as the substrate campaign, and defines authority hierarchy (roadmap governs v0.2.x; trait-surface.md governs beyond v0.2.2).

- [resolved] [execution-roadmap.md:PR-2d acceptance + Quality Gate Summary] W-2 FIXED. PR-2d acceptance criterion now explicitly requires `./scripts/bench.sh --samples 10`. Quality Gate Summary table lists PR-2d alongside PR-4a-ii in the 10-sample row. CI vs. manual distinction is now documented.

- [resolved] [execution-roadmap.md:PR-4a-i + PR-4a-ii] W-3 FIXED. Split is implemented exactly as conceded. PR-4a-i is additive (Low risk, no gate); PR-4a-ii is behavioral (High risk, 10-sample gate). RetrievalStrategy trait now aligned with trait-surface.md §3.2 (`fn name()` + `async fn collect(ctx: &QueryContext) -> CandidateSet`). This simultaneously resolves R1's critical RetrievalStrategy incompatibility finding.

- [resolved] [execution-roadmap.md:PR-4c "Pre-split requirement"] W-4 FIXED. Explicit shared-helper audit requirement added before the split begins.

- [partial] [execution-roadmap.md: Quality Gate Summary] W-5 PARTIALLY FIXED. Gate results now directed to benchmark_log.csv via bench.sh. But baselines.json is still not explicitly named as the gate comparison source — it remains implied. The benchmark-harness spec replaces the fragile CSV-grep with baselines.json; the roadmap should say so.

- [resolved] [execution-roadmap.md:PR-1d] Admin.rs black hole FIXED. PR-1d is a complete Phase 1 PR with full scope table, 5-step execution order, file exceptions pre-justified, and module-decomposition.md §2d as the implementation reference.

### R1 Attack Findings — Status After Reforge

- [resolved] [execution-roadmap.md:Phase 3 dependency diagram] PR-3b's dependency on PR-2d is now explicit. PR-3c's dependency on PR-3b is now explicit. "Three parallel PRs" fiction gone.

- [resolved] [execution-roadmap.md:PR-3c "Trait scope note"] Conformance suite capability parameterization resolved. Suite covers common-subset only; AdvancedSearcher/PhraseSearcher excluded from the generic bound with explicit explanation.

- [resolved] [execution-roadmap.md:PR-4c scope table] Pipeline layout now correctly targets `sqlite/pipeline/` with all 8 files matching module-decomposition.md §2c. The `advanced/` fiction from R1 is gone.

- [resolved] [execution-roadmap.md: Risk Registry — items 6, 7, 8] Three unregistered risks from R1 are now registered: visibility cascade (Risk 6), PR-2b warning blocking chain (Risk 7), PR-1c ConnPool scope expansion (Risk 8). All have specific mitigations.

- [resolved] [execution-roadmap.md: Branch Naming and Rebase Protocol] New section with 15 bookmark names and explicit `jj rebase -b` commands for the serial chain. jj-colocated workflow documented.

- [resolved] [execution-roadmap.md: Quality Gate Summary] 10-sample gate manual vs. CI distinction explicitly documented. Implementers will not assume CI enforces the 10-sample requirement.

- [resolved] [execution-roadmap.md:PR-4c acceptance] File size exception carve-outs now explicit in the acceptance criterion: "with `admin/maintenance.rs` ~580 and `admin/welcome.rs` ~490 exceptions per `module-decomposition.md` §4". The contradiction from R1 is gone.

### Remaining Gaps After Reforge

- [important] [execution-roadmap.md:PR-3a acceptance criterion] bench_strategy binary vs. --strategy flag ambiguity REMAINS. PR-3a acceptance says `cargo run --release --bin bench_strategy -- --help`. The benchmark-harness spec defines the comparison feature as `--strategy <name>` on the existing `locomo_bench` binary (benches/locomo/main.rs) via a new `strategies.rs` registry. These are different implementations. An implementer following the roadmap acceptance criterion will create a separate binary; one following the benchmark-harness spec will add a flag. Fix: update PR-3a acceptance to match benchmark-harness.md §1 — reference `locomo_bench --strategy` and `--list-strategies` rather than a separate binary.

- [minor] [execution-roadmap.md:PR-1a acceptance criterion] Full-mode regression test gap REMAINS. Acceptance still only requires testing that Minimal mode returns 4 tools. A regression in Full mode (still returning all 19) is undetectable. Fix: add "Add a test asserting `McpToolMode::Full` (or the default mode) returns all 19 registered tools."

- [minor] [execution-roadmap.md: Quality Gate Summary] baselines.json not explicitly named as the gate comparison source. bench.sh currently uses fragile CSV-grep; the benchmark-harness spec replaces it with baselines.json. The roadmap should say: "The gate compares against `docs/benchmarks/baselines.json` (see benchmark-harness.md §4)." Fix: one sentence addition to Quality Gate Summary.

- [minor] [execution-roadmap.md: Phase 4 preamble vs. PR Summary Table] PR-4b scheduling inconsistency introduced by the reforge. Phase 4 preamble says "PR-4a-i and PR-4b can be opened in parallel after Phase 3 merges." PR Summary Table shows PR-4b with no dependency ("—"). These contradict each other. Since PR-4b (mcp_server.rs split) has no logical dependency on Phase 3, the PR Summary Table is correct and the preamble is wrong. Fix: change preamble to "PR-4b can be opened in parallel with Phase 2 or Phase 3 — it has no dependency on either."

- [nitpick] [execution-roadmap.md: Quality Gate Summary] Benchmark gate thresholds (2pp warn / 5pp fail) are applied uniformly. Structural-move PRs (PR-2c, PR-4c) theoretically warrant a tighter threshold since zero behavioral change should produce zero score delta. I defended this in R1 and maintain the defense: a 0pp gate on 2-sample data implies false precision; 5pp is the honest catastrophic-regression detector. The current thresholds are defensible. No change required.

### Readiness Assessment

**Phase 1 (PR-1a, PR-1b, PR-1c, PR-1d): READY TO DISPATCH.**
- PR-1a: clean, well-scoped. One test gap (Full-mode) is minor and can be caught in review.
- PR-1b: trivially safe.
- PR-1c: start with ConnPool spike (documented in Risk 8). If concurrent readers are not supported, re-scope before implementing. Do not dispatch implementation agent blind.
- PR-1d: excellent spec with step-by-step execution guide. Clean to dispatch.

**Phase 2 (PR-2a through PR-2d): READY after Phase 1 validates.**
The serial chain is well-documented. Risk 7 (PR-2b warning blocking chain) is registered but not fully mitigated — the mitigation says "investigate before proceeding" which is correct but requires judgment from whoever is dispatching. Make sure the dispatching agent has authority to pause and assess rather than blindly continuing.

**Phase 3 (PR-3a, PR-3b, PR-3c): READY after Phase 2.**
The bench_strategy binary ambiguity in PR-3a should be resolved before dispatching (one sentence edit to the roadmap). Otherwise clean.

**Phase 4 (PR-4a-i, PR-4a-ii, PR-4b, PR-4c): READY after Phase 3.**
PR-4a-ii is High risk — the 10-sample gate is manual and must be explicitly surfaced to the implementing agent. PR-4b can technically start earlier (Phase 2 parallel) per the PR Summary Table, but the prose says Phase 3. Resolve the inconsistency, then decide when to schedule it.

**Overall confidence: 85%.** Three minor edits would push it to 93%+:
1. Fix PR-3a acceptance criterion to reference `locomo_bench --strategy` (one sentence).
2. Fix Phase 4 preamble PR-4b scheduling claim (one phrase).
3. Add baselines.json reference to Quality Gate Summary (one sentence).
