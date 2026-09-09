---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Selected-runtime HTTP request diagnostic

## Scope and decision

Reconciled the open PR queue, branch list, selected runtime/model contracts,
blueprint ownership and the existing `wire-lfm25-production-ingestion` todo.
PR #446 was the only open PR and was merged after exact-head CI, approval and
empty review-thread checks. Work starts from main
`eaecbd39ac31493aae0b07a6195e4eb20ef48d50`. The former root ROADMAP.md is absent;
node-linked Cairn artefacts remain the work authority. No new todo was created.

This is the HTTP-observation part of the existing request/template diagnostic,
not completion of P1. A bounded loopback recorder observes actual CLI request
bodies; Python does not reconstruct runtime prompts or add memory semantics.
The benchmark and test nodes already own the new files. Existing capture,
strict JSON, dataset identity and atomic-write helpers are reused.

Production Rust, prompt v1, model defaults, MCP, scoring, datasets and archived
baselines are unchanged. No model is run and no quality improvement is claimed.
Server-rendered templates remain explicitly unmeasured. The next boundary is
pinned-server template inspection followed by separately disclosed constrained
output, held-out and rule-only comparisons before production wiring.

## Test-first evidence

The new test module was written before the diagnostic existed. Its first run
failed importing the absent module. This is a missing-implementation RED boundary,
not a claimed behavioral regression in production code. The completed local suite
then ran 14 tests: 13 passed and the actual-Rust integration test was explicitly
skipped because this container has no Rust toolchain or release binary.

Tests cover exact body bytes/hash, annotation canaries, excluded authorization
headers, malformed/duplicate JSON, malformed HTTP, oversized headers/bodies,
stalled and drip-fed absolute deadlines, duplicate calls retaining the first
body, nonzero exits after HTTP, input aliases, input quotas and changed binaries.
No test fixture's placeholder is treated as a generated answer.

CI's existing Python matrix discovers the suite on Linux Python 3.10/3.13 and
macOS Python 3.13. The release `llm` job separately supplies the real CLI and
checks all development cases. It writes evidence before assertions so failures
are retained. Exact-head CI, actual request observations, external review and
merge results are recorded on the linked PR, not presumed in this document.

## Review and limitations

The recorder accepts only the selected provider's fixed-length loopback POST.
Header/body reading and the subprocess have whole-operation time bounds, not
only per-read socket timeouts. Captured bytes survive JSON or producer failures;
header values and backend stderr do not enter evidence. Source text is stored
intentionally and the documentation warns against unapproved private datasets.

Observed binary hashes and caller-declared source revisions are distinct.
An isolated home is not a hostile-code sandbox. Replacing the endpoint/model
alias is disclosed, and the artifact has no model results or scorecard fields.
No conclusion about server rendering or model capability follows from a passing
request diagnostic. A separate model experiment is still required.

Local `cairn scan`/`hook all`, Rust build, clippy and full tests cannot run in this
container: no Rust/Cairn toolchain is installed and outbound DNS is unavailable.
The focused contract/todo were read directly; the existing remote Cairn and
repository CI gates are required before merge. No gate is weakened or bypassed.
