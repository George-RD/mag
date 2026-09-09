---
node: mag.runtime.memory.models
status: open
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

The next slice belongs to this todo. Inspect the actual answer-blind HTTP request
and server-rendered chat template through the selected CLI/runtime before another
prompt change. Then consider a non-repairing schema-constrained comparison that
separates output-shape compliance from extraction quality and discloses its
request semantics. Model limitations are an alternative explanation, not an established
cause. Preserve the baseline and negative controls; do not tune or weaken the
scorer until results look favorable. Qualify improvements on additional held-out
cases before production promotion.

Then wire LFM2.5 1.2B into the chosen production write path behind explicit
enable/disable configuration, observable health, bounded structured output,
and safe rule-based fallback. No cloud credential may be required. The P1 gate
still requires improved memory usefulness over rule-only extraction within
local latency/RAM budgets. This todo is ready for diagnosis, not default enablement.
