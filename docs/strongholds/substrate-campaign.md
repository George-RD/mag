# MAG Experimentation Substrate Campaign

> **Historical campaign record.** The merged substrate work remains useful design evidence, but it is not the current production roadmap.
> `dec.select-local-runtime-composition-root` rejects wholesale substrate promotion and retains it only during bounded migration and retirement.

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
