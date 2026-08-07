---
node: mag.runtime.memory.storage.sqlite
status: in_progress
created: 2026-08-07
---
# Make checkpoint saves atomic and return their saved outcome

The current checkpoint workflow counts matching records on a reader, inserts in a
separate write transaction, and then makes CLI and MCP query again to infer the
checkpoint number. Concurrent writers can choose the same number, and the second
query can observe another writer's checkpoint. Generic canonical deduplication
can also make `save_checkpoint` return a newly generated ID that was never stored.

## Acceptance criteria

- [ ] Number allocation and checkpoint insertion occur in one immediate SQLite
  write transaction, including across independent MAG processes/pools.
- [ ] The selected runtime returns a typed outcome containing the persisted
  `memory_id` and `checkpoint_number`.
- [ ] The existing string-returning `CheckpointManager::save_checkpoint` and
  `LocalMemoryRuntime::save_checkpoint` remain compatibility wrappers.
- [ ] Canonically deduplicated checkpoint saves return the existing persisted ID
  and number rather than a phantom candidate ID.
- [ ] CLI and MCP serialize the returned outcome directly and do not query
  `resume_task` after saving.
- [ ] Existing CLI and MCP output contracts remain byte/field compatible.
- [ ] A concurrent file-backed regression proves unique monotonic numbers from
  independent connection pools.
- [ ] Exact-head CI and Cairn pass before merge.

## Boundary

This slice preserves checkpoint content, metadata structure, TTL, priority,
resume ordering, CLI JSON, and MCP JSON. It does not remove canonical
checkpoint deduplication or change the public compatibility method.
