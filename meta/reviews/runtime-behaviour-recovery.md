---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-10
reviewer: OpenAI coding assistant
---
# Runtime-behaviour recovery

## Scope and source

Recover the eight non-generative observation families and 36-seed dataset from
Claude branch `a2bbb71c05be39272b7efff77197bd8a080f2574` onto main
`09c863459cfb5943a58ecde306d303244df69b12`. The original branch is retained.
Demos and source-checked documentation are separately reviewed in #450. Existing
PR #449 and its schema comparison are not modified.

The source runner used a private profile inconsistent with main. This port uses
`bge_small_en_v1_5_embedding_model`, splits the large driver/family modules,
reuses the existing percentile helper, and records persisted identity read-only.
It retains the original dataset bytes but rejects unsupported schema, wrong
annotation partitions and date overflows. Temporary database names cannot be
chosen by dataset keys. Missing RSS/latency/scores are null, and the misleading
mean/grade, legacy adapter choice, old CSV results, broad size exemptions and
stale Cairn cache/status changes are excluded.

The existing calibration todo is the owning work item. P0 remains complete;
this corpus is not the matched rule-only baseline needed to qualify P1.
Methodology and the preserved historical-note erratum are beside the evaluator.

## Test-first boundary

Test-only remote commit `f4e1c2d1556e8159bf9364bf433e40354cfc1cba` preceded
implementation. CI 34456165799, Test job 102802935573 failed at
`tests/runtime_behaviour_eval.rs:9`: `CARGO_BIN_EXE_memory_runtime_eval` was
absent. A separate rustfmt failure is not counted as the behavioral RED.

Nine process contracts cover original identity, unsupported schema, filename,
partition, date overflow, changed bytes, sanitized custom-source identity, placeholder
reporting and family selection. Unit coverage includes recovered metric cases,
dataset validation, private workspace cleanup, missing RAM, sample-aware
percentiles, checked dates and the shared BGE profile identity.

Local authoring has no Rust toolchain. Remote compilation, strict lint and
full exact-head CI are required; source reconstruction is not a test pass.
Model-output measurements, when run, must retain their original per-case
results instead of selecting a favorable rerun. No production-quality claim
follows from a passing regression suite.

## Metadata regression diagnosis

Run 34462869362 reproduces two test-contract failures at c347eb3: the shared
benchmark metadata helper intentionally reduces dataset paths to their filename.
Preserve that privacy behavior instead of overriding it. Corrected tests retain
the original digest assertion and add a custom-source/no-local-path check. That
new assertion fails first because custom data is incorrectly labelled repo-local;
the diagnostic now distinguishes user-supplied directories. No shared production
helper, extraction scorer or dataset byte is changed.

Run 34463306071 passes the recovered unit and nine process tests under both
no-default-features and the production embedding feature. Its strict Clippy
gate then identifies a nested date check and an allocated comparison path. Both
are simplified without suppressing lint or changing the checked date bounds.

## Verified recovery observation

Run 34464045279 passes 32 no-feature unit tests plus nine process tests,
33 default-feature unit tests plus nine process tests, and strict
all-target/all-feature Clippy. Its original BGE observation is retained
under `benches/runtime_behaviour/baselines/2026-09-10-bge-small/`, with
source/tree, byte digest and limitations. The evidence-integrity test
fails on the missing file before copying original verified bytes.
All eight families are measured; known failures remain visible.

## Supersession observer correction

An introspective source check found the inherited family reversing `SUPERSEDES`:
production `crud.rs` calls `add_relationship(old_id, new_id, ...)`, whereas the
old observer required new-to-old. Two tests store actual rows through
`LocalMemoryRuntime`, add each directed edge without a version-chain mutation,
and fail before the comparison is corrected. This isolates the edge signal
rather than letting the existing OR with version-chain evidence hide the error.
The original archived run remains byte-identical, with its invalid edge-detail
booleans explicitly qualified beside it. No production or dataset change.

## Independent review follow-up

Codex 3978075206 identified SQLite initialization inside async seeding. Two
current-thread regressions cover actual initialization and a locked metadata
read; the latter lets executor progress release a real SQLite transaction.
Both fail before moving initialization and metadata reads to spawn_blocking.
Join errors retain context. The runtime itself is unchanged.

Codex 3978075227 found a bare-relative-path hole in generic command metadata.
Split and equals-form process regressions fail before this diagnostic records
canonical typed options with the dataset redacted. Shared provenance fields and
the dataset digest remain; no production helper is changed.

Codex 3978075232 found contradictory question annotations. Two process tests
fail before validation requires references for answerable questions and none for
abstention controls. The original dataset and historical observation stay
byte-identical. The earlier edge-direction finding was independently fixed in
47a33b6 with two runtime-backed RED/GREEN tests.

Verification continuation: run 34558793292 reproduced both executor failures and all four privacy/annotation failures before applying fixes. The same tree then passed feature-free and default-feature diagnostic unit/process tests, strict all-target/all-feature Clippy, and formatting. Previous workbench runs stopped on missing/stale patch helpers, not successful verification; this run reuses edited_dataset to recompute valid manifest digests. Exact-head CI and fresh independent review remain required before merge.

## Fresh review: impossible annotations

Codex review of d57ba88 identified intersecting temporal present/absent keys and overlapping grouping membership (comments 3985717669 and 3985717674). Run 34559563024 first reproduces both process failures with valid manifest digests, then passes the complete default and feature-free evaluator tests, strict all-target/all-feature Clippy and formatting. A shared annotation-consistency helper preserves the question invariant and adds these two guards. No scorer, production code, original dataset or archived observation changes. Full exact-head CI and renewed independent review remain the merge boundary; the run artifact also records a once-only release BGE smoke at the resulting commit.
