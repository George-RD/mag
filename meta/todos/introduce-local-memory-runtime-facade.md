---
node: mag.runtime.entrypoints
status: open
created: 2026-07-29
---
# Introduce the local memory runtime facade

Create the additive, transport-independent composition root selected by
`dec.select-local-runtime-composition-root`. The working name is
`LocalMemoryRuntime`; choose a different implementation name only if the same
boundary remains clear.

## TDD acceptance criteria

- A failing integration test first proves the runtime can be constructed once and
  used for store, retrieve, search, and advanced-search without changing outputs.
- The facade owns or shares one embedder, one `SqliteStorage`, and the current
  capability delegates; it does not duplicate retrieval or storage semantics.
- The initial facade introduces no schema, stored-content, ranking, CLI, or MCP
  response change.
- Local stdio construction has no daemon, HTTP, cloud credential, or network
  dependency.
- Existing direct callers remain available for rollback while the facade is
  additive.
- Public-surface parity tests use hermetic paths and do not touch user data.
- Full CI and the pinned Cairn gate pass. Retrieval or scoring changes are out of
  scope; if they become necessary, the benchmark gate applies.
