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

The scorer consumes recorded outputs only. It neither loads models nor adds
production ingestion or MCP semantics. Metadata snapshots are recorded rather
than validated by a competing model-profile implementation. Canonical-label and
exact-source-set scoring is deliberately strict and is not a factual-entailment
judge or a broad model-quality claim.

## Recorded-output verification evidence

- TDD started at the missing-case boundary: one of two cases omitted must yield
  50% task success and one false negative. The new scorer's initial unimplemented
  boundary failed; the implemented boundary passes.
- Review found JSON exponent overflow (`1e9999`) bypassing rejection of nonfinite
  constants. A failing regression was added before fixing numeric decoding.
- Dataset review found an expected contradiction attribute absent from the
  producer instruction. A failing assertion now requires that canonical label
  vocabulary to be disclosed for both positive and negative contradiction cases.
- PR #441 review identified responsibility being labeled as ownership in the
  Arabic seed. The source now states ownership explicitly, with an observed
  failing fixture regression before the correction.
- PR #441 review also found deeply nested JSON escaping the CLI error path.
  A regression reproduced tracebacks for both dataset and run input before
  recursion failures were included in controlled exit-2 handling.
- PR #441 local Python 3.13 verification passed 29 hermetic tests, including CLI
  execution, all annotations round-tripping as reference outputs, invalid
  artifacts, incomplete runs, fabricated citations, and unmeasured performance.
- Four local behavioral mutations were rejected by assertions: removing missing
  cases from the denominator, accepting content without correct provenance,
  accepting duplicate output values, and fabricating zero latency.

## Answer-blind producer capture

`benches/memory_intelligence/capture.py` adds a bounded trusted CLI bridge.
Requests allowlist task, instruction, and immutable source memories with a
protocol version. Neither expected annotations nor label-bearing case IDs enter
the request. The parent associates each response with its case and retains
failed attempts without retries or repair. Each producer has a fresh working
directory; POSIX group cleanup terminates same-group descendants on success,
failure, and timeout. This is explicitly not a security sandbox.

The capture bridge reuses the scorer's strict JSON, validation, and atomic-write
helpers. It preserves supplied metadata and observes per-case process wall time,
not isolated model latency. It does not invent tokens, model load time, or RAM.
No MAG runtime producer or model-quality baseline is claimed by this slice.

Local Python 3.13 verification passed 23 new tests, including real subprocesses,
CLI artifacts, quota/deadlock cases, malformed responses, input-alias protection,
virtual-environment executable symlinks, and descendant cleanup. TDD reproduced
answer/case-ID leakage and the symlink-resolution failure before correction.
PR #442 review additionally reproduced false timeouts when a successful parent's
child retained inherited pipes; cleanup now observes parent exit before EOF and
preserves buffered output. Five behavioral mutations were killed by assertions:
leaking annotations, dropping failed attempts, disabling stream quotas, inventing
zero latency, and leaving descendants after a successful parent exit. Durable
scope and evidence are in `meta/reviews/memory-intelligence-producer-capture.md`.

The evaluation workflow runs both suites and dataset validation on Linux with
Python 3.10/3.13 and macOS with Python 3.13. Full repository CI and the pinned
Cairn gate remain required at the exact PR head before merge.

Source ownership is already covered by `mag.quality.benchmarks` (`benches/`) and
`mag.quality.tests` (`tests/`); no new production dependency edge is introduced.
Authoring used direct Cairn artefact inspection because this environment lacks a
runnable Cairn/Rust checkout; the repository's architecture gate is not waived.

## Remaining before completion

Implement a compatible producer through the selected CLI-first MAG runtime and
capture its actual profile/resource observations, then record a real local-model
baseline. Do not introduce a parallel Python or MCP memory implementation. Do
not feed expected annotations to the producer or count fixture success as model
evaluation. Preserve facts, entities, temporal references, relationships,
decisions, questions, status, grouping, contradictions, and provenance coverage.
Keep this todo in progress until the end-to-end runtime harness and baseline exist.
