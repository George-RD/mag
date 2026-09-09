---
node: mag.runtime.memory.models
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Schema-constrained comparison

Continues the existing P1 `wire-lfm25-production-ingestion` after merged #448.
No PR was open at selection. #448 exact-head CI, evaluation and Cairn passed;
both timeout review threads were resolved. The root `ROADMAP.md` is absent;
`docs/specs/local-first-roadmap.md` and accepted Cairn decisions are authoritative.
The original extraction failures do not identify a model or template root cause.

## Boundary and review

The repairing `complete_structured` helper also forces temperature zero, so it
cannot be reused for this comparison. A separate native-constraint trait method
returns raw text and fails by default for unsupported backends. The selected
runtime owns the fixed, answer-free schema and shared validation/bounds/redaction.
The CLI owns one explicit flag and profile disclosure. One shared OpenAI HTTP
exchange replaces duplicate request construction; the legacy repairing API keeps
its old temperature and fence handling. Default requests remain unchanged.

The schema is a requested output-shape constraint, not a semantic validator.
Neither fallback, repair, scorer changes, new prompt, model-default promotion,
production persistence nor MCP semantics are introduced.

## Test-first evidence

Test-only head `9fce2ad5623873b7b1b92862a802269c2e092995` adds actual-binary
schema/default profile assertions before implementation. CI run `34387235621`,
Test job `102586476091`, reached both tests after all existing test suites passed:
`--json-schema` was rejected as an unexpected argument, and the default output
mode was null rather than `unconstrained`. Both behavioral assertions failed
(exit 101). A separate rustfmt failure is not counted as behavioral RED.
No local Rust compiler is available.

Three local runner regressions first failed on the absent `json_schema` keyword.
After implementation all three pass, including actual fixture processes,
contradictory-profile rejection before server launch, and strict flag typing.
The complete local Python intelligence suite ran 113 tests in 64.507 seconds:
OK, one skipped (the actual-binary diagnostic needs a compiled MAG executable).
Wire regressions compare the entire request after removing only response_format,
retain fences/invalid output, and reject server failures without repair/fallback.
Runtime tests cover raw output, bounds, input validation, error redaction and
unsupported backend behavior. A legacy-provider test protects fence repair and
its historical temperature-zero request.

## Environment and remaining gates

Source snapshot run `34386923350`, artifact `10118045513`, produced checkout
`f30f2cc12809cc28dbcae59a180fd75f216969e7` and tree
`1a2ca6264a864c8182afb42ad66abf8f08d5700f`. Locally reconstructing the archived
Git tree and raw commit yielded those exact identities. This is source identity,
not a claim that Rust or the model ran locally. The temporary acquisition and
publication workflows must not remain in the final PR diff.

Cairn 0.9.0 `brief todo.wire-lfm25-production-ingestion` reports no matching todo,
although `status` lists the exact in-progress file and `bundle` resolves the
owning node. This command friction is recorded rather than treating the todo as
missing or creating another one. `next` reports existing architecture drift.

Full remote Rust/profile checks, Cairn scan/hooks, review, exact-head CI and any
measured model comparison remain separate gates; only completed results count.


## Completed verification and measured outcome

Implementation head `3b9bc46fae0f40b1fb374479f79c4d57489f9156` passed all eleven
CI jobs in run `34389894351`, all four evaluation jobs in `34389894343`, and
Cairn gate `34389894562`. The release `llm` profile ran 17 Rust tests plus the
17-test actual-binary request diagnostic without skips. The retrieval benchmark
classifier correctly returned false; the retrieval benchmark itself did not run.

Paired measurement run `34389969043` built the same MAG/server binaries once and
retained both original 14-case arms in artifact `10119587157`. The copied original
ZIP, exact source identity, settings, scorecards, limitations and decision are in
`benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu-schema/`.
Independent replay reproduces both scorecards exactly. Native schema raised
scorer-valid outputs from 11 to 13 and exact successes from 3 to 4, with only one
positive case solved. Eight positive cases still abstain; grouping and duplicate
contradiction labels remain failures. All three negative controls pass in both.
No production/default change follows from this result.

The archive-integrity regression first failed because the expected checked-in
archive was absent; after copying the original verified bytes it passed. This is
an evidence-integrity RED/GREEN boundary, not a claim of a model-quality threshold.
Final documentation/evidence changes require their own exact-head CI. Codex was
requested at the implementation head; completed review status belongs to the PR
record, not an assumed approval from an empty review-thread list.
