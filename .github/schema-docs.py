from pathlib import Path

def edit(name, old, new):
    p = Path(name)
    s = p.read_text()
    assert s.count(old) == 1, (name, s.count(old))
    p.write_text(s.replace(old, new))

def append(name, text):
    p = Path(name)
    p.write_text(p.read_text() + text)

edit('meta/contracts/memory.models.md', 'The selected runtime calls plain `LlmBackend::complete` once per request, never\nthe repairing structured-completion path.', 'The selected runtime calls `LlmBackend::complete` once per request by default.\nExplicit schema diagnostics use `complete_constrained` with the fixed runtime\noutput-shape schema. Neither evaluation mode uses the repairing\nstructured-completion path. Native constraints retain the same prompt and\ndecoding configuration; unsupported backends fail without silent fallback.')
edit('meta/contracts/runtime.entrypoints.md', 'The runtime owns validation and one plain completion attempt.', 'The runtime owns validation and one completion attempt. Plain mode remains the\ndefault; `--json-schema` selects `produce_intelligence_with_schema`, adding only\nthe fixed native output constraint. `--describe` records the requested mode and\nexact schema without claiming server enforcement.')
append('meta/contracts/quality.benchmarks.md', '''
The local baseline runner accepts an explicit `--json-schema` comparison arm.
It passes that mode to both CLI description and every answer-blind attempt,
rejects a contradictory description before server launch, and preserves the
configured schema beside existing verified-artifact evidence. Request semantics
change only by adding native output constraints; prompt, scorer and dataset do
not change. See `benches/memory_intelligence/SCHEMA_COMPARISON.md`. A schema-valid
completion is not evidence of correct extraction or production readiness.
''')
edit('meta/todos/wire-lfm25-production-ingestion.md', 'Next consider a non-repairing schema-constrained comparison that separates\noutput-shape compliance from extraction quality and discloses its changed\nrequest semantics.', '''PR #449 implements an explicit, non-repairing `--json-schema` diagnostic through
this same CLI/runtime. It requests the fixed output-shape schema while keeping
prompt v1 and decoding settings, and preserves malformed output and failures.
The local baseline runner forwards the flag to description and every attempt;
unsupported constraints do not silently fall back. See
`benches/memory_intelligence/SCHEMA_COMPARISON.md` and
`meta/reviews/memory-intelligence-schema-comparison.md`.

Next measure the disclosed schema-constrained comparison, separating
output-shape compliance from extraction quality.''')
append('benches/memory_intelligence/LOCAL_BASELINE.md', '''
## Explicit native output constraints

Add `--json-schema` to select the non-repairing comparison arm. The default
remains unconstrained. Use different output paths and preserve both attempts;
see [SCHEMA_COMPARISON.md](SCHEMA_COMPARISON.md) for changed request semantics,
interpretation limits and production qualification gates.
''')
Path('benches/memory_intelligence/SCHEMA_COMPARISON.md').write_text('''# Native schema-constrained comparison

This is an opt-in diagnostic under `wire-lfm25-production-ingestion`, not a
production profile or an output-repair path. It asks whether a native output
shape constraint changes schema compliance **and** extraction quality. The
original baselines, prompt v1, dataset and independent scorer stay unchanged.

Build the documented producer with `cargo build --locked --release --features llm`.
`mag intelligence-produce --json-schema --describe` reports the exact fixed
schema without contacting a model. Omit `--json-schema` for the original request
semantics. The description labels both modes and keeps unknown model identity
fields null. A reported schema is a requested constraint, not proof the server
supports or enforces it.

The selected runtime validates the same answer-blind request and makes one call.
The OpenAI-compatible adapter adds only `response_format`: messages, model,
temperature and maximum tokens remain the same. The generic backend's native
constraint method fails explicitly when unsupported; it never falls back to
plain completion or the older repairing `complete_structured` API. The shared
HTTP exchange retains that older API's zero temperature and fence repair for
existing callers, but evaluation does not use them.

The schema contains only output structure and minimal string/array bounds. It
contains no canonical-label enums, case IDs, known source-ID enums, or answers.
An empty `items` array is legal. Nonblank values, uniqueness, known provenance,
source support and exact task success still belong to the unchanged scorer.
Native shape compliance alone cannot justify production enablement.

## Capture

Use the existing `local_baseline.py` command documented in `LOCAL_BASELINE.md`,
adding `--json-schema` for the constrained arm and a distinct output path.
Both `--describe` and every actual case receive the flag. A contradictory
producer description fails before the local server starts. No producer attempt
is retried or repaired; failed and malformed outputs remain in the denominator.
The configured producer profile, including its schema, is preserved in the run.
The existing provider's surrounding-whitespace trim still applies in both modes.

For a comparison, build MAG and the pinned server once, verify the same GGUF,
and run each arm once on the same host. Keep each original artifact, every
failure, binary hashes, arguments and hardware context. Separate server
lifecycles and fixed execution order can affect performance; do not claim a
randomized trial, hermetic build, cold-cache benchmark or isolated model time.
Use the same independent scorer and negative controls. This development seed is
not held out. Additional held-out, rule-only and local resource-budget
qualification still gate production ingestion.
''')
Path('meta/reviews/memory-intelligence-schema-comparison.md').write_text('''---
node: mag.runtime.memory.models
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
''')
