---
node: mag.runtime.memory.storage.sqlite
review_type: agent_introspective
date: 2026-09-06
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

## Reproducible evidence

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
