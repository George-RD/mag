# Native schema-constrained comparison

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
