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

The next slice belongs to this todo. Diagnose rendered requests/chat templates,
structured-output handling, and empty positive extractions using the selected
CLI/runtime. Model limitations are an alternative explanation, not an established
cause. Preserve the baseline and negative controls; do not tune or weaken the
scorer until results look favorable. Qualify improvements on additional held-out
cases before production promotion.

Then wire LFM2.5 1.2B into the chosen production write path behind explicit
enable/disable configuration, observable health, bounded structured output,
and safe rule-based fallback. No cloud credential may be required. The P1 gate
still requires improved memory usefulness over rule-only extraction within
local latency/RAM budgets. This todo is ready for diagnosis, not default enablement.
