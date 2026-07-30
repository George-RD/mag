---
node: mag.runtime.entrypoints
status: in_progress
created: 2026-07-29
started: 2026-07-30
---
# Migrate CLI command families to the local runtime

The local facade is merged. Migrate production callers in bounded slices.

Move production CLI callers through the selected runtime in bounded command
families rather than one large switch:

1. ingest/process, with byte-for-byte regression coverage for the current
   `processed: ` stored-content behaviour;
2. retrieve/delete/update and other basic CRUD;
3. search, semantic/advanced retrieval, graph, session, and administration.

Each PR must begin with a failing caller-level parity test, keep the previous
delegate easy to restore, and preserve JSON/stdout behaviour. Retrieval,
scoring, reranking, or storage changes require the benchmark gate and are not
permitted to hide inside a caller migration.

## Progress

- [x] `ingest` and `process` route through `LocalMemoryRuntime`; one shared helper
  now owns validation and `MemoryInput` assembly while stdout and the current
  `processed: ` stored-content contract remain pinned by a caller-level test.
- [x] `retrieve` routes through `LocalMemoryRuntime`; the same caller-level test
  preserves exact JSON and stored content, with red evidence against the legacy
  entrypoint path before the production change.
- [x] `delete` routes through `LocalMemoryRuntime`; exact JSON and boolean deletion
  semantics are pinned at the caller and facade levels. Temporary compatibility
  `Pipeline` assembly now has one helper instead of duplicate entrypoint code.
- [x] `update` routes through `LocalMemoryRuntime`; exact compact JSON, direct
  content updates, and every supported field remain pinned at the caller and
  facade levels. Optional metadata parsing is shared without collapsing omitted
  metadata into an explicit empty object.
- [x] `list` and `relations` route through `LocalMemoryRuntime`; pagination,
  filters, exact compact JSON, relationship output, and direct-storage parity
  remain pinned at caller and facade levels.
- [x] `search`, `semantic-search`, and `advanced-search` route through
  `LocalMemoryRuntime`; filters, limits, exact compact JSON, scores, and the
  compatibility-sensitive advanced-search metadata remain pinned at caller and
  facade levels.
- [x] `recent` routes through `LocalMemoryRuntime`; filters, ordering, exact
  compact JSON, and compatibility-pipeline parity remain pinned. The compatibility
  `Pipeline` is now private to the runtime composition root.
- [x] `phrase-search` routes through `LocalMemoryRuntime`; phrase matching,
  filters, ordering, exact compact JSON, and direct-storage parity remain pinned.
- [x] `version-chain` and `similar` route through `LocalMemoryRuntime`; version
  ordering and metadata augmentation, similarity scores, exact compact JSON, and
  direct-storage parity remain pinned.
- [x] `traverse` routes through `LocalMemoryRuntime`; exact compact empty-graph
  JSON, the selected-runtime marker, one-hop node hydration, hop, edge type,
  weight, and direct-storage parity remain pinned.
- [x] `checkpoint` and `resume-task` route through `LocalMemoryRuntime`; checkpoint
  numbering, content, metadata, project/query filters, exact compact JSON, resume
  markdown, and direct-storage parity remain pinned.
- [ ] Migrate reminder, lesson, profile, welcome, and administration commands.
