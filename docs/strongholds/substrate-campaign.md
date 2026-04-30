# MAG Experimentation Substrate Campaign

**Status**: Phase 1 DONE, Phase 2 DONE, Phase 3 DONE, Phase 4 DONE — campaign complete (all 15 PRs merged)
**Campaign Workspace**: `../mag-substrate` (jj workspace `substrate-campaign`)
**Generated**: 2026-04-14 | **Completed**: 2026-04-30

## Vision

MAG v0.2 is a Rust core exposing stable traits for Storage, Retrieval, Fusion, Scoring, Lifecycle, and Consolidation, with current SQLite + FTS5 + ONNX + multi-factor scoring preserved as the default reference implementation, and a benchmark harness that can run any swapped implementation against LoCoMo-10 with zero-regression as the merge gate.

## Specs

| Spec | Path | Status |
|------|------|--------|
| Module Decomposition | `docs/specs/module-decomposition.md` | Validated (Phase 2+3 confirmed design) |
| Trait Surface Design | `docs/specs/trait-surface.md` | Draft |
| Benchmark Harness | `docs/specs/benchmark-harness.md` | Draft |
| Execution Roadmap | `docs/specs/execution-roadmap.md` | Approved (3 Palantir rounds) |

## Reconnaissance Strongholds

| Recon | Path | Status |
|-------|------|--------|
| Source Tree Map | `docs/strongholds/recon-source-tree.md` | Complete |
| Scoring Pipeline | `docs/strongholds/recon-scoring-pipeline.md` | Inline (not persisted) |
| Dead Code Audit | `docs/strongholds/recon-dead-code.md` | Complete |
| Test Infrastructure | `docs/strongholds/recon-test-infra.md` | Complete |
| Existing Docs | `docs/strongholds/recon-existing-docs.md` | Inline (not persisted) |

## Completed PRs

| PR | Title | Merged | Benchmark |
|----|-------|--------|-----------|
| #289 | docs: substrate campaign specs & strongholds | 2026-04-14 | N/A |
| #290 | docs: update MCP tool count 16→19 | 2026-04-14 | N/A |
| #291 | perf: parallelize sub-query fan-out with JoinSet (#121) | 2026-04-14 | 91.5% PASS |
| #292 | refactor: split admin.rs into admin/ subdirectory | 2026-04-14 | N/A |
| #293 | feat: wire McpToolMode::Minimal to filter tool list | 2026-04-14 | N/A |
| #294 | fix: address review comments on #291, #293 | 2026-04-14 | N/A |
| #295 | style: fix rustfmt in advanced.rs | 2026-04-14 | 91.5% PASS |
| #296 | refactor: extract ScoringStrategy trait (PR-2a) | 2026-04-14 | N/A (additive) |
| #297 | refactor: extract Reranker trait boundary (PR-2b, #119) | 2026-04-14 | 91.5% PASS |
| #299 | refactor: sqlite/mod.rs structural extraction (PR-2c) | 2026-04-14 | PASS |
| #300 | refactor: inject ScoringStrategy into SqliteStorage (PR-2d) | 2026-04-14 | PASS |
| #301 | feat: strategy comparison benchmark harness (PR-3a) | 2026-04-14 | N/A |
| #302 | feat: in-memory HashMap backend MemoryStorage (PR-3b) | 2026-04-14 | N/A |
| #303 | test: shared backend conformance suite (PR-3c) | 2026-04-14 | N/A |
| #305 | docs: add field-level doc comments to QueryContext / FullPipelineStrategy (PR-4a-i) | 2026-04-14 | N/A (additive) |
| #306 | refactor: split mcp_server.rs into mcp/ module directory (PR-4b) | 2026-04-14 | N/A |
| #307 | feat: add KeywordOnlyStrategy + intent-based dispatch (PR-4a-ii) | 2026-04-14 | 10-sample PASS |
| #317 | refactor: split advanced.rs into pipeline/ subdirectory (PR-4c) | 2026-04-14 | PASS |
| #318 | fix(search): address eight deferred retrieval/lock bugs (post-roadmap) | 2026-04-14 | N/A |

## Phase 4 Status

DONE. Both parallel chains landed:
- **4a-i + 4a-ii** (RetrievalStrategy trait + KeywordOnlyStrategy dispatch) — `src/memory_core/retrieval_strategy.rs` exposes `RetrievalStrategy`, `FullPipelineStrategy`, and `KeywordOnlyStrategy`; `advanced_search` dispatches via intent.
- **4b + 4c** (mcp_server.rs → `src/mcp/`, advanced.rs → `src/memory_core/storage/sqlite/pipeline/`) — `mcp_server.rs` is gone; `advanced.rs` residual is 675 lines (orchestration body) that delegates to eight `pipeline/` modules.

## Key Corrections from Source Inspection

1. **sqlite/mod.rs trait impls already distributed** — The 19 trait implementations are already in submodules (crud.rs, search.rs, etc.). mod.rs problem is mixed concerns (struct, cache, relationships, I/O), not trait monolith.
2. **Tool count is 19, not 16** — 15 legacy + 4 Wave 2 facades in TOOL_REGISTRY.
3. **McpToolMode::Minimal is stubbed but not wired** — src/mcp_server.rs:54-56 explicitly says so.
4. **Codebase is clean** — Zero dead code, zero unused deps, only 1 TODO (#121).
5. **28 existing traits** — More mature trait surface than assumed. Refactor extends, not rewrites.

## Phase Summary

| Phase | Version | Theme | PRs | Critical Path |
|-------|---------|-------|-----|---------------|
| 1 | v0.1.9 | Clean House | 4 (parallel) | PR-1c (benchmark gated) |
| 2 | v0.2.0 | Scoring Decoupling + Structural Cleanup | 4 (sequential) | PR-2b → PR-2c → PR-2d |
| 3 | v0.2.1 | Prove the Substrate | 3 | PR-3a ∥ PR-3b → PR-3c |
| 4 | v0.2.2 | Exercise & Decompose | 4 | PR-4a-i ∥ PR-4b → PR-4a-ii → PR-4c |

## Quality Gates

- Every PR: `prek run` (fmt + clippy + tests)
- Scoring/search PRs: `./scripts/bench.sh --gate` (>2pp warn, >5pp fail)
- Phase 2 exit: LoCoMo-10 within 1pp of baseline (91.5%)
- Phase 3 exit: Conformance suite green on both backends
- Phase 4 exit: Two retrieval strategies benchmarked, god modules decomposed

## What "Done" Looks Like

- 7 clean trait boundaries with reference implementations preserving v0.1.9 behavior
- Benchmark harness running any strategy combination against LoCoMo-10
- Proof-of-life alternatives for at least 2 of the 7 planes
- MCP facade stable at 4+15 tools, protecting downstream integrations
- LoCoMo-10 retrieval accuracy >= 90.1%
- No file > 500 lines without documented justification

## Parked Items (Not This Campaign)

- Closed-loop recall tracking / self-improving scoring (Step 7+)
- MAGMA multi-graph (semantic + temporal + causal + entity)
- Closed vs open schema for memory content
- Agent-native curation vs machine-structured ingestion
- PostgreSQL / Redis backends (after conformance suite proven)
- Nightly benchmark workflow
- Testing skill implementation (docs/specs/testing-skill.md — parked until Phase 2 stabilizes)

## Execution Protocol

Every PR and spec follows the **debate→reforge cycle**:

1. **Implement** — Nazgul in worktree isolation, spec path + acceptance criteria
2. **Gate** — `prek run` + benchmark gate (if scoring/search touched)
3. **Review** — Deploy Watchers (code-reviewer, shadow-hunter) OR Palantir debate (for specs)
4. **Escalation rule** — Any finding above nitpick → fix → re-review. Repeat until clean.
5. **Forge PR** — Use `/forge-pr` skill for commit → push → PR creation. Returns PR number.
6. **Siege loop** — After PR is created, start a review-polling loop:
   ```
   /loop 3m /siege-tick <PR-URL>
   ```
   This runs every 3 minutes, checks GitHub for new review comments, and acts on them
   (fixes requested changes, replies to comments, pushes updates). The loop continues
   until the PR is approved and merged. If reviews are slow (no activity for 30+ min),
   widen the interval to avoid burning context.
7. **Merge immediately** — Once approved, merge via `gh pr merge <PR> --squash`. Don't batch PRs.
8. **Rebase dependents** — After merge: `jj git fetch && jj rebase -b <downstream-bookmark> -d main`
9. **Next PR** — Check `docs/specs/execution-roadmap.md` dependency graph. Dispatch the next
   unblocked PR(s) in the phase. If multiple are unblocked, dispatch in parallel worktrees.

### Concrete Phase 1 Dispatch Commands

Phase 1 has 4 independent PRs. Dispatch all in parallel:

- **PR-1a** (wire McpToolMode::Minimal): `isolation: "worktree"`, bookmark `fix/mcp-minimal-mode`
- **PR-1b** (fix AGENTS.md tool count): goblin-level, no worktree needed
- **PR-1c** (parallelize sub-queries #121): `isolation: "worktree"`, bookmark `perf/parallel-subqueries`
  - **IMPORTANT**: Begin with ConnPool concurrent-reader spike (Risk 8). If ConnPool doesn't support it, rescope before implementing.
- **PR-1d** (split admin.rs → admin/): `isolation: "worktree"`, bookmark `refactor/admin-split`

After all 4 merge → Phase 2 begins (PR-2a ∥ PR-2b, then PR-2c, then PR-2d).

See `docs/specs/execution-roadmap.md` §Branch Naming for full jj bookmark conventions.

## Palantir Debate History

| Round | Issues Found | Fixed | Result |
|-------|-------------|-------|--------|
| R1 | 3 critical + 10 important | 13/13 | Reforged |
| R2 | 5 important + 3 minor | 8/8 | Reforged |
| R3 | 1 minor | 1/1 | **Approved** (92-93% confidence) |

Strongholds: `palantir-r{1,2,3}-attack.md`, `palantir-r{1,2,3}-defense.md`, `palantir-r3-final.md`
