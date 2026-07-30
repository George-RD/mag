---
node: mag.runtime.entrypoints
status: done
created: 2026-07-29
started: 2026-07-30
completed: 2026-07-30
---
# Introduce the local memory runtime facade

`LocalMemoryRuntime` is now the additive, transport-independent composition root
selected by `dec.select-local-runtime-composition-root`. It owns the current
compatibility pipeline and a clone of the same SQLite handle for extended local
capabilities; those clones share the underlying pool, embedder, and caches.

## Result

- Store, retrieve, basic search, and advanced search delegate to the existing
  compatibility-sensitive implementations without changing their outputs.
- `new_with_path` supports hermetic local construction; the facade has no daemon,
  HTTP, cloud credential, or network dependency.
- Existing CLI, MCP, and direct SQLite callers remain unchanged for rollback and
  later command-family migration slices.
- The duplicated in-memory SQLite constructor implementations were consolidated
  in the storage module. Their unit-test, regression, and benchmark callers remain
  intact; only the binary module copy's false dead-code warning is suppressed.

## TDD acceptance criteria

- [x] A failing integration test first proved the facade was absent before
  implementation.
- [x] One constructed runtime preserves store, retrieve, search, and
  advanced-search outputs against the legacy delegates.
- [x] The facade shares one embedder and one underlying SQLite pool rather than
  duplicating retrieval or storage semantics.
- [x] No schema, stored-content, ranking, CLI, or MCP response changed.
- [x] Local construction has no daemon, HTTP, cloud credential, or network
  dependency.
- [x] Existing direct callers remain available while the facade is additive.
- [x] Public-surface parity tests use a temporary database and do not touch user
  data.
- [x] Full CI and the pinned Cairn gate pass.

## Verification

- Red: CI run `30513153353` passed formatting, then failed tests and Clippy on
  `unresolved import mag::LocalMemoryRuntime` before implementation existed.
- Green: CI run `30513888204` passed the all-feature Rust suite, formatting,
  Clippy, smoke coverage, npm installation, Python 3.8/3.13 wrappers, version
  checks, and all installer-integrity variants. The main Rust target reported
  649 passing tests, with every integration target also green.
- Cairn: pinned 0.9 gate run `30513888194` passed after `cairn scan` recorded the
  new entrypoint path and interface hash `054eca3561bee0c1` with zero scan errors.
- The benchmark gate correctly skipped because no watched retrieval, scoring,
  reranking, or storage-query implementation changed.
