---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Local baseline readiness and action pins — PR #445

## Scope and ownership

Reconciled main `00257a39a185ae166dd0ec9fc6e8f20dea5113a6`, the open PR queue,
branches, Cairn status/next, the existing ingestion todo, and its binding
rationale. No open PR was displaced. Two review findings arrived on #444 after
its merge: comments `3964608484` and `3964608490`. Both matched the current code.
This follow-up belongs to the existing P1 diagnostic work, not a new roadmap todo.

The benchmark and test nodes already own these files. No runtime interface,
production dependency, prompt, scorer, model default, or MCP behavior changes.
The recorded 3/14 baseline and its zero positive matches are preserved unchanged.
P1 remains open for request/template/structured-output diagnosis and held-out
qualification before production wiring.

## TDD evidence

Test-only head `af9c62af98f1cb24d89cd079b5b8a3fdea68285a` precedes the fix.
Evaluation run `34356699193` failed the added tests on Linux Python 3.10/3.13
and macOS Python 3.13. The inspected Linux job `102483084878` passed all 68
existing tests, then recorded both 0.75-second endpoint delays as readiness
timeouts, the incorrect shared-budget assertion, and all five mutable action
references. These are behavioral/assertion failures, not missing dependencies.

With the fix, local Python verification passed all 71 tests in two bounded
invocations: 17 baseline tests and 54 capture/evaluation/profile/replay tests.
A first combined local invocation hit the execution tool's time limit and is not
counted as completed verification; its fixture process was explicitly stopped.

The existing dripping-body deadline, process cleanup, incorrect alias, failed
attempt, provenance, input-integrity and archived-score replay tests remain
unchanged. Fixture success is not new model-quality evidence.

## Implementation and review

One named six-second cap covers the existing five-second socket timeout and
interpreter startup. Both endpoints still receive only the time remaining on
the same overall startup deadline. The existing capture supervisor, retry
classification and cleanup are reused; no alternate HTTP loop was added.

All five baseline workflow action references use actual upstream commit SHAs.
The annotated rust-cache tag was peeled to commit
`6323deb102c322ba6fcbdcafc7e3dddab59af2b6`, not pinned to its tag object. The
existing weekly GitHub Actions Dependabot entry already covers maintenance.
Runner images and the stable Rust toolchain remain mutable; these pins do not
claim a hermetic build or source-to-binary attestation.

The temporary read-only source-inspection workflow is removed from the final
tree. Its run `34355928356` exported tracked source only for offline tests.
Final exact-head repository CI, evaluation matrix, review findings and merge
result are recorded on PR #445. Rust/Cairn execution is remote, not claimed local.

## Cairn query friction

In inspection run `34355928356`, Cairn 0.9.0 `brief` with the documented
`todo.wire-lfm25-production-ingestion` identifier returned no match and exit zero,
although `status` found the existing filename-based todo. This matches the
filename compatibility issue recorded in #444. The canonical todo was read
directly without duplicating or renaming it. `next` also reported existing
blueprint drift; the initial status log had 32 findings and zero errors. No
architecture gate was weakened to suppress those findings.
