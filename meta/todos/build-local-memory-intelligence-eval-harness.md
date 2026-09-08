---
node: mag.quality.benchmarks
status: in_progress
created: 2026-07-28
---
# Build Local Memory Intelligence Eval Harness

The production composition root and role-aware model-profile prerequisite are
complete. Continue this existing P0 todo rather than opening a duplicate roadmap
item. `docs/specs/local-first-roadmap.md` remains the sequencing reference.

## Recorded-output foundation

`benches/memory_intelligence/` now contains a versioned 14-case synthetic
development dataset and a standard-library Python scoring CLI. All ten planned
task categories are represented, with negative controls, multi-source provenance,
and an Arabic-to-English fact case.

The scorecard records the supplied exact profile snapshot and its digest,
embedding-space identity (explicitly null when no embedding space participated),
dataset digest, producing code revision, and measurement context. It reports
schema validity, exact content and grounded precision/recall/F1, task success,
p50/p95 observed latency, observed tokens, load time, and peak RAM. Missing
measurements are null and aggregates include sample counts. Missing cases remain
failures; malformed output and incorrect provenance cannot earn task success.

This slice consumes recorded outputs only. It neither loads models nor adds
production ingestion or MCP semantics. Metadata snapshots are recorded rather
than validated by a competing model-profile implementation. Canonical-label and
exact-source-set scoring is deliberately strict and is not a factual-entailment
judge or a broad model-quality claim.

## Verification evidence

- TDD started at the missing-case boundary: one of two cases omitted must yield
  50% task success and one false negative. The new scorer's initial unimplemented
  boundary failed; the implemented boundary passes.
- Review found JSON exponent overflow (`1e9999`) bypassing rejection of nonfinite
  constants. A failing regression was added before fixing numeric decoding.
- Dataset review found an expected contradiction attribute absent from the
  producer instruction. A failing assertion now requires that canonical label
  vocabulary to be disclosed for both positive and negative contradiction cases.
- Local Python 3.13 verification passes 28 hermetic tests, including CLI execution,
  all annotations round-tripping as reference outputs, invalid artifacts,
  incomplete runs, fabricated citations, and unmeasured performance.
- Four local behavioral mutations were rejected by assertions: removing missing
  cases from the denominator, accepting content without correct provenance,
  accepting duplicate output values, and fabricating zero latency.
- `.github/workflows/memory-intelligence-eval.yml` runs these tests and dataset
  validation on Python 3.10 and 3.13. Full repository CI and the pinned Cairn gate
  remain required at the exact PR head before merge.

Source ownership is already covered by `mag.quality.benchmarks` (`benches/`) and
`mag.quality.tests` (`tests/`); no new production dependency edge is introduced.
Authoring used direct Cairn artefact inspection because this environment lacks a
runnable Cairn/Rust checkout; the repository's architecture gate is not waived.

## Remaining before completion

Add reproducible output and resource capture through the selected CLI-first
runtime, then record a real local model baseline with actual profile metadata.
Do not feed expected annotations to the producer or count reference-fixture
success as model evaluation. Preserve facts, entities, temporal references,
relationships, decisions, questions, status, grouping, contradictions, and
provenance coverage as live capture is added. Keep this todo in progress until
that end-to-end harness and baseline exist.
