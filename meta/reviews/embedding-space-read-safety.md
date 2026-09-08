---
node: mag.runtime.memory.storage.sqlite
review_type: agent_introspective
date: 2026-09-08
reviewer: ChatGPT
---
# Embedding-space read and cache safety

## Scope and review result

This completes the read/cache requirement left open by PR #435 in the existing
`implement-embedding-space-migration` todo and issue #89. Reviewed startup,
migration, semantic/similar reads, advanced-search phases, hot/query caches, and
the shared runtime callers. This is an introspective self-review, not independent
approval or an executed slash-command `/code-review`.

The CLI remains canonical through `LocalMemoryRuntime`; MCP and the model defaults
are unchanged. No ranking weights, retrieval strategy, model selection, public
runtime method, or second storage/model composition path is added.

## Invariants and alternatives checked

- An identity preflight followed by unguarded reads is insufficient. Each vector,
  index, and hydration read uses a SQLite transaction whose first reads verify
  identity and generation. Metadata and data therefore share one snapshot.
- Advanced search spans pooled connections. Every candidate, fusion, graph, and
  decomposition phase must match the pool's immutable startup generation. The
  final live validation is the read's completion/linearization point and also
  covers query-cache hits and hot-cache short circuits. A migration committing
  after that point is ordered after the read; the guard does not promise to stop
  migration until the caller receives the response.
- Identity alone does not distinguish A-to-B-to-A migrations. A persisted u64
  generation advances in the existing migration transaction with the vectors and
  identity. Missing/invalid generations fail closed during reads; exhaustion
  rejects migration and rolls back rather than wrapping. Opening an older
  compatible database initializes generation zero. Dry-run, no-op, and failed
  migration preserve the generation.
- Query/hot caches are cleared when a guarded read fails. All three hot-cache
  refresh paths share one snapshot-verified helper. A refresh already holding an
  older valid snapshot may finish later, but its stale runtime cannot pass final
  validation or serve that cache. The runtime never silently adopts a new model
  or generation.
- Blocking SQLite work stays in blocking workers; no SQLite transaction or
  cache lock crosses an await. Returning cached results now needs a metadata
  read. That is a deliberate correctness cost, not a zero-overhead claim.
- Metadata-only operations and raw retrieval remain available. Generation tracks
  embedding migration, not every ordinary cross-process write; existing cache
  TTL behavior for ordinary writes is unchanged. Restoring/replacing a database
  under running processes remains unsupported.

## TDD boundary

Test-only head `e99964f313c3306d86a39d4aad89011f1485f51b` was run before the
production patch in [RED run 34158983897](https://github.com/George-RD/mag/actions/runs/34158983897).
Both no-default-feature BLOB fallback and explicit sqlite-vec runs required
`1 passed; 4 failed`, with these runtime assertions rather than compiler errors:

- `stale semantic reads must fail visibly`
- `stale cached advanced reads must fail visibly`
- `round-trip migration must not revive an old generation`
- `migration racing a semantic read must fail visibly`

The fixed model returns identical normalized vectors for different identities;
identity, not vector shape or value, is the distinguishing condition. A query
counter proves the advanced-search fixture actually warmed and hit its cache.
Query embedding and reranking are paused with bounded channels, not timing sleeps,
to place migration at deterministic boundaries.

Additional tests cover migration after a read snapshot is acquired, migration
between advanced candidate/fusion phases, query/hot cache clearing, missing and
malformed generation metadata, no-op/dry-run/backup generation, and exhaustion
rolling back already-changed vectors. The SQLite snapshot regression inspects
both BLOB and sqlite-vec data when the feature is enabled.

## Verified implementation and cleanup

[GREEN run 34159459046](https://github.com/George-RD/mag/actions/runs/34159459046)
verified a patched worktree and then published source commit
`5f94f333dcd6a27db43ff43df04b4a41bf68afdd`. It passed:

- 1,026 tests across the all-feature Rust targets, including doc tests;
- 17 focused tests in each storage configuration: eight migration, six stale
  runtime, two snapshot, and one cache-invalidation regression;
- Rustfmt and strict all-target/all-feature Clippy;
- the repository retrieval benchmark and Cairn scan/hooks.

The two-sample LoCoMo gate produced 91.3% word overlap against the recorded 90.1%
ten-sample baseline and passed. The differently sized samples do not establish
an improvement, and the runner's 162-day stale-methodology warning is retained.
No benchmark baseline or generated results file is changed by this PR.

That intermediate Cairn run had no errors, 34 warnings, and three informational
findings. One new warning came from the expanded inline advanced-search tests;
cleanup moves that same test module to `advanced_tests.rs` under the existing
SQLite owner. One new informational finding came from the temporary patch
carrier; cleanup removes it and the temporary workflow. The original 33 warnings
and two informational findings concern existing ownership/provenance and module
size issues. Final cleaned-head Cairn results must be checked separately rather
than inferred from this intermediate run.

The temporary job had narrowly scoped contents-write permission to publish its
verified source. It is removed before merge; ordinary CI retains read-only
contents permission. Cleanup changes test placement and documentation, not
production behavior. The PR records final exact-head CI, benchmark, smoke,
wrapper, installer, and architecture results before merge.

Artifacts `read-fence-red` and `read-fence-green` have finite retention. The
committed tests, source commit, commands, assertions, and this review are the
durable reproduction record. The green artifact digest is
`sha256:9b0f40d4781d4e54730e401be9669b9396a75b5b8f8769ede1930b8fd0f691bd`.

## Rerun commands

```bash
cargo test --no-default-features --test reembed_migration --test reembed_stale_runtime
cargo test --no-default-features --features sqlite-vec --test reembed_migration --test reembed_stale_runtime
cargo test --no-default-features --lib embedding_snapshot
cargo test --no-default-features --features sqlite-vec --lib embedding_snapshot
cargo test --no-default-features --lib generation_rejection
cargo test --no-default-features --features sqlite-vec --lib generation_rejection
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/bench.sh --gate
cairn scan
cairn hook all
```

Cairn navigation friction was recorded with `cairn feedback`: the prefixed
`cairn brief todo.implement-embedding-space-migration` found no match although
status and the owning storage bundle exposed the todo. Work continued from the
owning bundle rather than inventing a replacement task. The pre-existing root
roadmap reference resolves to `docs/specs/local-first-roadmap.md`; live task
status remains in Cairn's native todos.

The supported operating procedure remains stop all sharing processes, migrate,
then restart. See `docs/re-embedding.md`.

## PR #439 review follow-up: 8 September 2026

The [review TDD run](https://github.com/George-RD/mag/actions/runs/34246491404) requires runtime assertion failures for the populated
hot-cache refresh race and both deduplicated FTS fixtures in BLOB and sqlite-vec
builds before applying the fix. Refresh now releases its old snapshot and reader
before checking the live generation; failures clear actual cached entries. A
private refresh callback provides a deterministic interleaving without sleeps or
global test hooks. Earlier run 34245431412 stopped on a new SQL count fixture type
error; run 34246052335 proved RED and 56 GREEN checks but stopped on an unused
import in the temporary mutation harness. Neither compile failure was accepted as
a behavioral regression. The harness and portable artifact filenames were fixed.

Existing routing tests now count query embeddings: keyword and blank queries
skip vector work, while natural-language queries perform it. FTS fixtures use 121
distinct rows and prove the desired match is outside the unfiltered 100-candidate
limit. Mutation checks deliberately break each routing branch and move date
filtering after LIMIT; the strengthened assertions must fail. All mutations are
restored before engineering gates. Migration and read joins have bounded timeouts.

The parallel-candidate review finding does not require a second generation token:
ConnPool already pins one immutable generation for both reader snapshots, fusion,
and decomposition. The snapshot regression now also rejects a second reader after
migration while the original snapshot remains open. The final result/cache guard
remains required; these guards provide a validation boundary, not a promise to
prevent a migration immediately after validation. Stop/migrate/restart remains
the supported operational procedure.

Focused tests in both storage configurations, the full all-feature suite, Rustfmt,
strict Clippy, and the repository retrieval benchmark pass before source publication.
Cairn scan/hooks follow this note. This is worktree evidence, not final-head CI.
Final cleaned-head CI and review disposition are recorded on PR #439. This direct
self-review is not an independently executed slash-command /code-review. Artifacts
have 14-day retention; test names and repository commands remain durable.
