---
node: mag.runtime.memory.models
status: in_progress
created: 2026-07-28
---
# Wire LFM2.5 Production Ingestion

The composition-root and P0 evaluation-harness prerequisites are implemented.
PR #444's measured baseline is a usable diagnostic boundary, not a passing
production profile: 3/14 exact successes, all negative controls, and zero positive
matches. Its original attempts and measurements are preserved in
`benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu/`.

PR #445 addresses the post-merge baseline review findings: slower healthy
readiness responses retain the overall startup deadline, and all baseline
workflow actions use immutable commit pins. See
`meta/reviews/local-baseline-readiness-followup.md`. These are diagnostic-tool
fixes, not improved model-quality evidence; the original baseline is unchanged.

PR #446 rejects the extraction-oriented prompt-v2 candidate: 2/14 exact
successes, 4/14 schema-valid outputs, and zero positive matches. One negative
control regressed. The original 3/14 baseline remains unchanged; prompt v1 is
restored. The failed artifact and a patch reconstructing its exact producing tree
are retained under the dated `-prompt-v2` baseline directory. The common pins do
not establish identical server binaries; both observed hashes remain disclosed.
See `meta/reviews/memory-intelligence-prompt-v2.md`.

The HTTP-observation slice now has a bounded loopback recorder through the actual
selected CLI, with exact body bytes and failed attempts retained. It returns a
labelled placeholder, not model output, and never runs a scorer. See
`benches/memory_intelligence/REQUEST_DIAGNOSTIC.md` and
`meta/reviews/memory-intelligence-request-diagnostic.md`. P1 remains in progress;
this is diagnostic tooling, not evidence of improved model quality.

PR #448 adds the server-template replay diagnostic. All 14 original #447 request
bodies were rendered once by the pinned server; system/user content, Arabic text
and the assistant prefix are retained. The original observation ZIP, source audit
and limitations are preserved under
`benches/memory_intelligence/diagnostics/2026-09-09-template-v1/` and
`meta/reviews/memory-intelligence-template-diagnostic.md`. This weighs against
message loss in that replay, not against every possible integration failure.
The original recorder's server-template field and each replay's generation-prompt
field remain null: no historical generation prompt or token IDs were observed.

PR #449 implements an explicit, non-repairing `--json-schema` diagnostic through
this same CLI/runtime. It requests the fixed output-shape schema while keeping
prompt v1 and decoding settings, and preserves malformed output and failures.
The local baseline runner forwards the flag to description and every attempt;
unsupported constraints do not silently fall back. See
`benches/memory_intelligence/SCHEMA_COMPARISON.md` and
`meta/reviews/memory-intelligence-schema-comparison.md`.

The same-host paired comparison is now preserved under
`benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu-schema/`.
Native constraints improved scorer-valid outputs from 11/14 to 13/14, but solved
only one of eleven positive cases (Arabic owner extraction). Both arms passed
all three negative controls. Grouping remains wrong and duplicate contradiction
labels remain invalid. Same binary/model hashes and independent scorecard replay
are verified. This is not passing held-out or production qualification.

Next establish the rule-only comparator and additional held-out qualification
boundary before claiming useful improvement. Keep this model/mode diagnostic-only.
Do not rewrite the prompt based on an assumed template fault or keep tuning the
same development seed until results look favorable. Preserve every baseline and
negative control; do not repair outputs or weaken the scorer. Model limitations
remain an alternative explanation, not an established cause.

Then wire LFM2.5 1.2B into the chosen production write path behind explicit
enable/disable configuration, observable health, bounded structured output,
and safe rule-based fallback. No cloud credential may be required. The P1 gate
still requires improved memory usefulness over rule-only extraction within
local latency/RAM budgets. This todo is ready for diagnosis, not default enablement.
