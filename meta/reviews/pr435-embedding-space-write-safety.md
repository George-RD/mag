---
node: mag.runtime.memory.storage.sqlite
review_type: agent_introspective
date: 2026-09-07
reviewer: ChatGPT
---
# PR #435: write safety and pinned CLI composition

## Scope and result

Reviewed the recovered implementation and its callers across `main.rs`,
`LocalMemoryRuntime`, the embedding model/profile adapter, SQLite schema,
migration, store/update/batch paths, and the public regression tests. This is a
self-review, not an independent reviewer approval. The unavailable slash-command
runner was not represented as an executed `/code-review`.

The bounded change protects vector writes after migration and makes the CLI's
migration target the same pinned BGE profile as ordinary runtime construction.
It deliberately does not claim completion of live read/cache invalidation.

## Findings addressed

- The identity fence now runs inside the vector-writing transaction, before
  content/vector mutation or dedup side effects. Batch and checkpoint storage
  share the fenced store primitive. Metadata-only updates remain permitted.
- Migration rechecks its source identity after acquiring the writer slot, before
  taking the backup and replacing vectors. Competing migrations cannot silently
  use an identity observed before a different migration committed.
- The original profile factory accepted a custom 384-dimensional ONNX embedder
  and attached BGE's pinned metadata. A regression reproduced that defect; the
  factory now requires the private default constructor's checksum configuration.
- `ReembedOptions` is imported only in real-embedding builds. The internal
  profiled adapter is compiled only when production or test callers need it.
- The CLI profile regression isolates HOME, USERPROFILE, and MAG_DATA_ROOT. The
  stale-write fixture uses normalized, identical vectors so identity is the only
  distinction between source and target spaces.
- Duplicate identity helpers and the unusable legacy migration wrapper are
  removed. Temporary source/patch workflows and staging files are not retained.
  Normal CI uses strict Clippy and runs both minimal-feature migration targets;
  the Cairn job runs scan as well as the architecture hooks.

## Committed regressions and historical run evidence

Run `34024534055`, artifact
`pr435-implementation-evidence-41e2d5a8e7a8fef6ec18c553178f53c693040451`, contains
`original-red.log` (stale store unexpectedly succeeded), `profile-red.log`
(custom model unexpectedly received BGE identity), and `tests.log` (1,000 tests
passed with the implementation and guard). The minimal-feature import failure is
also preserved rather than described as a successful gate.

Run `34024862652` applies the corrected feature gating and passes the focused
all-feature and no-default-feature migration regressions before creating source
commit `be295658da3759d42cbacfde60f9f1b9c17e1780`. PR #435 records final-head
verification after cleanup; intermediate patched-worktree runs are not a
substitute for that final CI. Artifact logs have finite retention, so this file
preserves the failure assertions, commands, implementation SHA, and scope.

Commands: `cargo test --all-features`; `cargo test --no-default-features --test
reembed_migration --test reembed_stale_runtime`; `cargo fmt --all -- --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; `cairn scan`; and
`cairn hook all`. Product startup remains subject to the normal CLI/MCP smoke,
wrapper, and installer jobs. Benchmark applicability is determined by the
repository-owned classifier, not an independent workflow path list.

## Remaining issue: live read and cache fencing

`SemanticSearcher::semantic_search` embeds with the runtime's model and reads
vectors without checking the persisted identity in the same snapshot.
`AdvancedSearcher::advanced_search` can return its process-local query cache
before reading the database. This pre-existing gap is not fixed by write fences.
Do not close #89 or mark `todo.implement-embedding-space-migration` done.

The operating procedure in `docs/re-embedding.md` therefore requires stopping
all runtimes before migration and starting new ones afterward. The next slice
belongs to the existing todo: stale semantic and cached-query regressions,
snapshot/generation-safe reads, cache invalidation, and benchmark verification.


## Review follow-up: 7 September 2026

The cached pinned-artifact path created a nested Tokio runtime even when no
network operation was needed. The regression
`memory_core::embedder::artifact_regressions::cached_pinned_artifacts_work_inside_current_thread_runtime`
failed on the pre-fix source with `Cannot start a runtime from within a runtime`.
The verification run [review TDD and engineering gates](https://github.com/George-RD/mag/actions/runs/34102423945) applied only the
tests first, required that specific runtime failure (not a build failure), and
then applied the fix. Its source/staging head was `2b997a7d2063f201cc242b5e93f45d4253cc6a97`; this is patched-worktree
evidence, not a claim that that head already contained the production fix.

The run passed all 14 artifact regressions, 7 migration regressions in each
feature configuration, 1,014 all-feature Rust tests, strict Clippy, Rustfmt, a
fresh release-build CLI/MCP smoke, and Cairn scan/hooks. The repository classifier
returned `false`, so the retrieval benchmark itself was not run. The final push
step failed because the runner token could not delete workflow files; no test or
engineering gate failed. Its verified commit
`e1a46d12b9fd9a89504ca5879b7c700ed846ee68` was recovered through the GitHub connector.
Final cleaned-head CI remains a separate prerequisite, recorded in PR #435.

Verified caches are checked synchronously without creating a runtime. Cold or
corrupt-cache downloads invoked through the synchronous compatibility API use a
scoped worker when a Tokio handle is present. The worker owns and drops its
runtime outside the caller's async context; plain synchronous callers keep the
direct path. Production async entrypoints should continue using `spawn_blocking`.

Fourteen committed, hermetic `artifact_regressions` tests cover cached loads in
current-thread/multithread Tokio, plain sync and `spawn_blocking`; cold downloads
in both Tokio flavors and plain sync; bad-cache replacement; post-download
checksum rejection and removal; failed recovery; re-verification after model or
tokenizer replacement; external-data checksums; and unpinned-cache compatibility.
They use an independent loopback HTTP fixture, not production model downloads.
Re-run with `cargo test --all-features --lib artifact_regressions`.

CI now declares `permissions: contents: read`. The strict Clippy command remains
unchanged. The `manual_filter` failure was not reproducible on the reviewed head,
and the verification run passes without the allowance. The duplicate permission
reviews describe the same fixed issue. The ordinary smoke job explicitly builds
the checked-out source before running the smoke script, so an old cached release
binary cannot bypass rebuilding.

The proposed directory-only checksum memo was not added: it would weaken the
on-disk replacement check. Hashing occurs at session initialization/reload, not
per query; no measured performance regression was supplied. The committed
replacement regression protects this intentional correctness trade-off.

Historical numeric run IDs above are GitHub Actions runs, not agent-session IDs.
Their retained logs are supporting observations, not permanent reproducibility
assets. The committed regressions, named commands, and final exact-head CI links
in PR #435 are the repeatable evidence. No claim of independent slash-command
review or completion of live read/cache safety is made.
