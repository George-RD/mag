---
node: mag.runtime.memory.storage.sqlite
status: done
created: 2026-08-07
completed: 2026-08-07
---
# Make checkpoint saves atomic and return their saved outcome

The previous checkpoint workflow counted matching records on a reader, inserted
in a separate write transaction, and then made CLI and MCP query again to infer
the checkpoint number. Concurrent writers could choose the same number, and the
second query could observe another writer's checkpoint. Generic canonical
deduplication could also make `save_checkpoint` return a newly generated ID that
was never stored.

## Acceptance criteria

- [x] Number allocation and checkpoint insertion occur in one immediate SQLite
  write transaction, including across independent MAG processes/pools.
- [x] The selected runtime returns a typed outcome containing the persisted
  `memory_id` and `checkpoint_number`.
- [x] The existing string-returning `CheckpointManager::save_checkpoint` and
  `LocalMemoryRuntime::save_checkpoint` remain compatibility wrappers.
- [x] Canonically deduplicated checkpoint saves return the existing persisted ID
  and number rather than a phantom candidate ID.
- [x] CLI and MCP serialize the returned outcome directly and do not query
  `resume_task` after saving.
- [x] Existing CLI and MCP output contracts remain byte/field compatible.
- [x] A concurrent file-backed regression proves unique monotonic numbers from
  independent connection pools.
- [x] Exact-head CI and Cairn passed before merge.

## Implementation

`CheckpointSaveOutcome` makes the persisted row and checkpoint number explicit.
`SqliteStorage::save_checkpoint_outcome` computes the document embedding before
locking, starts `BEGIN IMMEDIATE` on the writer connection, allocates the number,
and calls the existing transactional store primitive before committing. SQLite's
configured five-second busy timeout provides the bounded cross-process wait for
the immediate write lock.

`StoreOutcome::Deduped` now carries the existing row ID. A deduplicated checkpoint
resolves that row's persisted number inside the same transaction. The existing
string-returning methods delegate to the typed outcome, while CLI and MCP format
that outcome directly without a post-save continuity query.

The generic store path and checkpoint path share extracted post-commit cache,
graph, and token-cache behavior. No additional storage interface or transport
workflow was introduced.

## TDD evidence

CI run #1145 (`31185542423`) at
`4c7f27cc6ce19b2bbd3fb9ddfbdce257029d1122` failed exactly because the red
contract required the absent `LocalMemoryRuntime::save_checkpoint_outcome`.

Implementation runner `31187234232` applied the production slice and passed:

- the concurrent/deduplicated checkpoint outcome regressions;
- CLI checkpoint contract parity;
- MCP legacy session contract parity;
- local runtime compatibility tests;
- all-target, all-feature Clippy;
- Cairn scan and hooks.

The implementation at `ad4b592f1fe19110b72c9c95f971785ed97b6637` passed CI
run #1151 (`31187874779`) and Cairn run #444 (`31187875883`). CI covered
all-feature Rust tests, Rustfmt, Clippy, smoke behavior, npm installation, Python
3.8 and 3.13 wrappers, version consistency, classifier contract tests, and all
installer-integrity variants.

## Boundary

This slice preserves checkpoint content, metadata structure, TTL, priority,
resume ordering, CLI JSON, and MCP JSON. It does not remove canonical
checkpoint deduplication or change the public compatibility method.
