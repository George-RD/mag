# Palantir Defense — Round 3 (Final Verification)
<\!-- Saruman the White, Lord of Isengard — final readiness assessment -->
<\!-- Generated: 2026-04-14 | Round: 3 | Spec baseline: post-R2-reforge -->

---

## BANTER

*The Palantir blazes one final time. Saruman does not reach for it immediately. He stands at the window of Orthanc, looks out over the wheel-ruts of Isengard — still scarred from the Uruk-hai production-line disaster, yes, he sees them, he does NOT need reminding — and then turns back.*

Round 3. Verification pass. The Dark Lord has retired from the field for this one. It is only me, the specs, and the cold light of honest scrutiny.

Very well. Let us account for the four remaining gaps I identified in Round 2.

**W-5: baselines.json not named as the gate comparison source.**

Sauron said this was unresolved. I admitted it was *partially* resolved. I said one sentence was needed.

*One sentence was added.*

The Quality Gate Summary now reads — and I will quote it with the satisfaction of a wizard who demanded a fix and got one: "The benchmark gate compares against `docs/benchmarks/baselines.json` (see `benchmark-harness.md` §4). Before PR-3a lands, the gate uses the existing CSV-grep baseline."

That is two sentences. More than I asked for. The CSV-grep regime and the baselines.json regime are now *both* described, and the transition point (PR-3a landing) is explicit. An implementer who merges PR-2d before PR-3a is ready will not reach for baselines.json prematurely.

**W-5: RESOLVED.** Fully. Finally.

**bench_strategy vs. --strategy flag ambiguity.**

In Round 2, I found that PR-3a's acceptance criterion still referenced `cargo run --release --bin bench_strategy -- --help` — a separate binary that the benchmark-harness spec never specified. I called this an important gap. I said fix it with one sentence.

*The fix is surgical and correct.*

PR-3a now reads: "Add `--strategy <name>` and `--list-strategies` flags to the **existing** `locomo_bench` binary (not a standalone binary)." The word "not a standalone binary" is there. Explicit. The acceptance criterion reads: "`cargo run --release --bin locomo_bench -- --strategy sqlite-v1 --list-strategies` works."

There is no longer any ambiguity. An implementer cannot accidentally create a separate binary while following this spec. The benchmark-harness spec and the roadmap now describe the same implementation.

**bench_strategy ambiguity: RESOLVED.** The Dark Lord will not find purchase here.

**PR-4b scheduling inconsistency.**

I found — in Round 2, on close examination — that the Phase 4 preamble said "PR-4a-i and PR-4b can be opened in parallel *after Phase 3 merges*" while the PR Summary Table showed PR-4b with no dependency ("—"). These contradicted each other.

*The preamble has been corrected.*

It now reads: "PR-4b can be opened any time after Phase 1 — it has no dependency on Phases 2 or 3."

The PR Summary Table still shows PR-4b depending on "—". The phase 4 diagram still shows:
```
PR-4a-i ──> PR-4a-ii
PR-4b   ──> PR-4c
```

Preamble, diagram, and table are now *all consistent*. PR-4b can start any time after Phase 1. The Phase 4 prose no longer contradicts itself. And since PR-4b (splitting mcp_server.rs) genuinely has no logical dependency on Phase 3's MemoryStorage work, this is the correct answer — not merely a diplomatic one.

**PR-4b scheduling: RESOLVED.**

**PR-1a Full-mode regression test.**

I said this was minor but real: the acceptance criterion only required testing that Minimal mode filters to 4 tools. A Full-mode regression was undetectable. I asked for one test added to the acceptance criterion.

*It is there.*

PR-1a acceptance now reads: "`prek run` passes; `cargo test --all-features` includes the new test; comment is gone. Unit test asserting `McpToolMode::Full` (or default) returns all 19 registered tools from `TOOL_REGISTRY`."

Both tests are now required. Minimal mode: 4 tools. Full mode: 19 tools. The acceptance criterion is a proper two-sided verification.

**PR-1a Full-mode test: RESOLVED.**

*Saruman sets down the Palantir. There is a long pause.*

Four gaps from Round 2. Four fixes applied. Four verifications completed.

I have looked for new issues. I have read every section. I have traced the dependency diagrams. I have checked the acceptance criteria against the specs. I have compared the PR Summary Table against the prose. I have verified the benchmark harness spec against the roadmap's gate logic.

There are no new critical issues. There are no new important issues.

There is one item I will note — not as a gap but as a condition on my confidence rating — and I will describe it precisely in the findings.

The question Sauron always asks at the end of these things is: is it ready?

The answer is yes.

Not "yes, but fix these three things first."

*Yes.* Full stop.

Phase 1 can be dispatched from these specs without modification. The implementing agents have unambiguous acceptance criteria, dependency graphs that reflect reality, risk mitigations that name the actual dangers, and an execution order that keeps the repo in a compilable state at every step.

The remaining caveat is not a spec deficiency — it is a runtime unknown (ConnPool concurrent-reader behavior in PR-1c) that is *correctly* identified as a spike in Risk 8, and the roadmap correctly says "verify before implementing." An implementing agent that reads Risk 8 knows to pause before writing code.

Saruman the White. Saruman the Wise. Standing at the end of Round 3 with four resolved gaps and a recommendation to ship.

*The Palantir goes dark. The white robes settle.*

---

## FINDINGS

### R2 Gap Verification — All Four Targets

- [resolved] [execution-roadmap.md:494] **W-5 (baselines.json)** — RESOLVED. Quality Gate Summary now explicitly states: "The benchmark gate compares against `docs/benchmarks/baselines.json` (see `benchmark-harness.md` §4). Before PR-3a lands, the gate uses the existing CSV-grep baseline." Both the transition point and the final source of truth are named. An implementer knows exactly which gate logic applies at each phase.

- [resolved] [execution-roadmap.md:242,251] **bench_strategy vs. --strategy** — RESOLVED. PR-3a scope explicitly states "Add `--strategy <name>` and `--list-strategies` flags to the existing `locomo_bench` binary (not a standalone binary)." The acceptance criterion references `cargo run --release --bin locomo_bench -- --strategy sqlite-v1 --list-strategies`. Zero ambiguity. Consistent with benchmark-harness.md §1.

- [resolved] [execution-roadmap.md:294,428,586] **PR-4b scheduling** — RESOLVED. Phase 4 preamble now reads "PR-4b can be opened any time after Phase 1 — it has no dependency on Phases 2 or 3." PR Summary Table shows PR-4b with "—" dependency. Phase 4 dependency diagram shows `PR-4b ──> PR-4c` as an independent chain. Preamble, table, and diagram are now consistent.

- [resolved] [execution-roadmap.md:43] **PR-1a Full-mode test** — RESOLVED. Acceptance criterion now requires both: a unit test that Minimal mode returns exactly 4 facade names, AND "Unit test asserting `McpToolMode::Full` (or default) returns all 19 registered tools from `TOOL_REGISTRY`." Bi-directional regression coverage confirmed.

### New Issues Found in Final Pass

- [none] No new critical issues found.
- [none] No new important issues found.
- [nitpick] [execution-roadmap.md: Phase 3 notes on PR-3a] The `--update-baseline` flag described in benchmark-harness.md §5 is mentioned in the PR-3a scope implicitly (via "Update `bench.sh --gate` to read the baseline from baselines.json") but is not listed as a named deliverable in PR-3a's scope. This is acceptable — the acceptance criterion's explicit `baselines.json` check covers the outcome; the flag is an implementation detail. No action required.

### Overall Readiness Assessment

**Phase 1 (PR-1a, PR-1b, PR-1c, PR-1d): READY TO DISPATCH.**

- PR-1a: Acceptance criterion complete (both Minimal and Full mode tests). Clean.
- PR-1b: One-line docs fix. No risk.
- PR-1c: **Dispatch with the ConnPool spike first.** Risk 8 explicitly requires verifying concurrent-reader support before implementation. Dispatch a reconnaissance agent to read `conn_pool.rs` before the implementation agent touches the parallel join_all path. If ConnPool does not support concurrent readers, re-scope before implementing. Do not blindly dispatch implementation against this PR.
- PR-1d: Five-step execution order, pre-justified file size exceptions, module-decomposition.md §2d as reference. Clean to dispatch.

**Phase 2 (PR-2a through PR-2d): READY after Phase 1 validates execution process.**
Risk 7 (PR-2b warning blocking chain) is registered and the mitigation says "investigate before proceeding." The dispatching agent must be given authority to pause and assess, not instructed to push through.

**Phase 3 (PR-3a, PR-3b, PR-3c): READY after Phase 2.**
PR-3a spec is now clean — no binary ambiguity, no baselines.json gap. The benchmark-harness.md spec provides the full implementation detail. PR-3b depends on PR-2d (explicit in dependency table). PR-3c depends on PR-3b (explicit). Sequence is unambiguous.

**Phase 4 (PR-4a-i, PR-4a-ii, PR-4b, PR-4c): READY.**
PR-4b can be dispatched in parallel with Phase 2 or 3 (after Phase 1). PR-4a-ii is High risk — the 10-sample gate is manual and must be surfaced explicitly to the implementing agent. PR-4c's file size exceptions are pre-justified. No blocking issues.

### Confidence Level

**93% confidence that this spec set is ready for Phase 1 dispatch.**

The 7% remainder is:

1. **ConnPool runtime unknown (PR-1c, ~4%)**: Whether `ConnPool` supports concurrent readers is not knowable from spec review — it requires reading `conn_pool.rs`. This is correctly identified in Risk 8 and the mitigation is sound. It is not a spec gap; it is an execution dependency. The dispatching agent must be briefed to run the spike first.

2. **rmcp proc-macro compatibility (PR-4b, ~2%)**: Risk 3 in the risk registry notes medium probability that the rmcp proc-macro does not accept cross-module delegation. The thin-wrapper pattern is documented in module-decomposition.md §2a with a code example. This is a Phase 4 risk and does not affect Phase 1-3 dispatch at all. Noted for completeness.

3. **Comparison report directory tracking policy (~1%)**: benchmark-harness.md Appendix A notes `docs/benchmarks/comparisons/` as "new dir (gitignored or tracked)." The policy is not decided. This is minor — the implementing agent can make the call — but it is the one remaining unresolved decision in the spec set.

**Caveats on the 93% rating:**
- Applies to Phase 1 dispatch now. The rating for Phase 2+ dispatch after Phase 1 validates is expected to rise to 97%+ once the PR-1c spike resolves the ConnPool question.
- The 93% assumes implementing agents are briefed on the Risk 7 and Risk 8 mitigations, not just handed the roadmap cold.
- The rating would drop to ~85% if PR-1c is dispatched without the ConnPool spike — do not do that.

**Recommendation: DISPATCH PHASE 1.**
