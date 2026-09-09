---
node: mag.runtime.memory.models
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Rejected prompt-v2 experiment — PR #446

## Scope and decision

Continued the only open PR, #446, against main
`badcd90ac996322ceddd11642ef1b6e9524f5e55`. Reconciled its source, branch,
review threads, completed diagnostic run and the existing P1 ingestion todo.
The repository's current task authority is Cairn `meta/todos/`; no root
`ROADMAP.md` exists and no parallel roadmap item was added. The model node owns
the diagnostic decision; existing benchmark/test ownership covers the files.

The measured candidate is rejected: schema validity fell from 11/14 to 4/14,
exact successes from 3/14 to 2/14, and positive content/grounded matches remained
zero. The explicit-supersession negative control regressed. All fourteen v2
attempts parse as JSON; ten still violate the item schema. Strings, arrays,
nulls and missing citations are not repaired into successful extractions.

Restore the runtime prompt, its version, CLI/runtime tests and model contract to
the main-branch v1 state. The final diff changes no Rust code, production behavior,
model default, scorer, dataset expectation, MCP semantics or active workflow.
Keep the original baseline byte-identical and P1 open; neither prompt qualifies
production ingestion. Rejection is a diagnostic result, not a model improvement.

## Durable measured evidence

Producing commit `9eb7c6048e09d61c111b3e2d7760f1a27503de65`, run
`34359655347`, artifact `10107880199`. The original 8,085-byte ZIP is retained in
`benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu-prompt-v2/`.
Its README records hashes, measurement methods, results, comparison limits and
replay commands. No model inference was repeated to select a favorable result.

The complete rejected source delta is saved as `experiment.patch`. Applying it
with `--unidiff-zero --index` at the recorded base yields tree
`cc7b6c059740429d61019bece4a8f5c3ad3f26b1`, exactly the measured source tree.
The original temporary model runner remains only inside this inert patch, not in
`.github/workflows/`. A later temporary read-only source snapshot (run
`34365048503`) supported offline review; it is also absent from the final tree.

The model pin, verified model bytes, dataset digest, claimed runtime-source
revision, producer decoding and non-ephemeral server settings match v1. The
observed MAG and server binary hashes do not. Do not claim a hermetic build or
isolated framing-only causal effect. This development seed is not held out, and
the result does not establish a model-family capability limit.

## Test-first evidence and local verification

Added replay regressions before importing the rejected artifacts. The original
baseline replay passed; three new tests failed because the rejected archive and
source patch were absent. This is a missing-evidence boundary, not a production
behavioral assertion failure. After importing the original ZIP, retaining the
source delta and restoring v1, all four replay/comparison tests passed.

The shared replay helper checks byte integrity, every case remaining present,
independent full-scorecard equality and unmeasured resource fields staying null.
Specific assertions retain the negative-control regression and invalid item
types. Comparison assertions check shared inputs without claiming equal binaries.
No second scorer or historical expected-answer repair was introduced.

Completed local Python 3.13 verification in two bounded invocations:

```bash
python3 -m unittest discover -s tests -p 'test_memory_intelligence_[bce]*.py' -v
# 57 tests passed
python3 -m unittest discover -s tests -p 'test_memory_intelligence_local_baseline.py' -v
# 17 tests passed
```

Total: 74 tests. A combined invocation exceeded the execution tool's 45-second
limit and is not counted as completed verification; no fixture/server processes
remained afterward. Exact-tree reconstruction also passed. Rust and Cairn are
unavailable in this local container; their execution is remote, not claimed here.
Final exact-head CI, evaluation matrix, Cairn scan/hooks, independent review and
merge verification are recorded on PR #446, separately from this self-review.

## Next boundary

Inspect actual answer-blind requests and the server-rendered chat template
before another prompt revision. A non-repairing constrained-schema comparison
can test output-shape compliance separately from extraction usefulness, but its
request semantics and all failures must stay visible. Positive held-out and
rule-only comparisons remain required before production enablement.
