---
node: mag.runtime.mcp
status: in_progress
created: 2026-07-29
started: 2026-08-01
---
# Migrate MCP to the local memory runtime

The runtime facade and all CLI command families are complete. MCP migration is
now in progress by bounded tool family so each slice can preserve protocol
behaviour independently.

Route both full and minimal MCP tool modes through the same transport-independent
runtime used by CLI. Start with failing protocol-level parity tests for tool
results, validation errors, and advertised tool sets. Preserve stdout exclusively
for MCP protocol traffic and keep diagnostics on stderr.

The stdio server remains the mandatory local baseline. This migration must not
introduce an HTTP, daemon, hosted-service, authentication, or cloud dependency.
Any future service adapter consumes the same runtime contract in a separate
milestone.

## Progress

- [x] Compose `LocalMemoryRuntime` once inside `McpMemoryServer` and route the
  unified `memory_admin` facade through it. Full and minimal tool advertisement,
  exact basic-health JSON, created-order list state, invalid-sort validation, and
  missing import-data validation remain pinned. The server temporarily retains its
  shared SQLite clone for MCP tool families that have not migrated yet.
- [x] Route the unified `memory_session` facade through the same runtime for
  welcome/info, checkpoints, reminders, lessons, and profile state. Minimal-mode
  stdio parity and shared-state visibility with the still-unmigrated `memory`
  facade remain pinned.
- [x] Route the five individual legacy session tools through the same server-owned
  runtime: `memory_session_info`, `memory_checkpoint`, `memory_remind`,
  `memory_lessons`, and `memory_profile`. Full-mode stdio parity pins the exact
  19-tool advertisement, welcome and protocol output, checkpoint continuity,
  reminder and profile state, lesson payloads, validation errors, and visibility
  of data written by the still-unmigrated legacy `memory_store` tool.
- [x] Route the unified `memory` facade through the same server-owned runtime for
  raw single and batch storage, retrieval, and deletion. Unit and minimal-mode
  stdio parity pin exact JSON, caller-supplied IDs, raw content, batch order, tool
  advertisement, and invalid-parameter errors.
- [x] Route the four individual legacy storage tools through the same server-owned
  runtime: `memory_store`, `memory_store_batch`, `memory_retrieve`, and
  `memory_delete`. Unit and full-mode stdio parity pin the exact 19-tool
  advertisement, raw content, caller-supplied IDs, batch order, validation errors,
  and shared visibility with the unified `memory` facade. Unified and legacy
  storage entrypoints now share one internal execution path per operation.
- [x] Route `memory_search` and `memory_list` through the same server-owned runtime
  for text, semantic, phrase, tag, and similar search plus created and recent
  listing. Unit and full-mode stdio parity pin the exact 19-tool advertisement,
  phrase and tag results, project and event filters, created totals, recent
  visibility, empty-tag behaviour, and validation errors. Search and list now
  share request-to-`SearchOptions` assembly, while the runtime exposes tag lookup
  publicly because the MCP binary consumes the library across a crate boundary.
- [x] Route `memory_update`, `memory_feedback`, `memory_relations`,
  `memory_lifecycle`, and the unified `memory_manage` facade through the same
  server-owned runtime. Unit and full-mode stdio parity pin exact update,
  feedback, relationship, traversal, lifecycle, and validation behaviour. The
  unified facade now adapts to the legacy action handlers instead of duplicating
  their execution, and `McpMemoryServer` no longer retains a second direct
  storage handle.

## Verification

- Red: commit `92723df` failed CI run `30690927339` because
  `McpMemoryServer` did not yet own a `LocalMemoryRuntime`.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, and the non-applicable benchmark gate passed in CI run
  `30691472875` at commit `c722388`.
- Cairn architecture, decision, and interface verification passed in run
  `30691472878` at the same commit.
- Red: commit `506b232` failed CI run `30692055111` only because
  `memory_session` still accepted `SqliteStorage` instead of the server-owned
  `LocalMemoryRuntime`.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, version consistency, and the non-applicable benchmark gate
  passed in CI run `30692921429` at commit `d04e929`.
- Cairn architecture, decision, and interface verification passed in run
  `30692921426` at the same commit.
- Red: commit `27387a7` passed Rustfmt and then failed CI run `30793307211` with
  twelve `E0308` type errors because the five legacy session functions still
  accepted `SqliteStorage` while the parity tests required `LocalMemoryRuntime`.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, version consistency, and the non-applicable benchmark gate
  passed in CI run `30794130536` at commit `97167bc`.
- Cairn architecture, decision, and interface verification passed in run
  `30794130526` at the same commit.
- Red: commit `0b3bb6a` passed Rustfmt and then failed CI run `30803891243` with
  eight `E0308` type errors because the unified `memory` facade still accepted
  `SqliteStorage` while the parity tests required `LocalMemoryRuntime`.
- Red: commit `fc10b80` passed Rustfmt and then failed CI run `30806477589` with
  seven `E0308` type errors because the four legacy storage functions still
  accepted `SqliteStorage` while the parity tests required `LocalMemoryRuntime`.
- CI run `30806950412` reached the migrated implementation but exposed a test-only
  contract error: the new legacy batch validation test hard-coded a maximum of 100
  while the shared MCP `MAX_BATCH_SIZE` contract is 1000. Commit `d817b46` binds
  the boundary test to that shared constant instead of duplicating the limit.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, version consistency, and the non-applicable benchmark gate
  passed in CI run `30818010017` at commit `d817b46`.
- Cairn architecture, decision, and interface verification passed in run
  `30818010004` at the same commit.
- Parity-backed simplification: commit `ef7d54f` replaced duplicated unified and
  legacy store, batch, retrieve, and delete execution with four shared helpers.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, version consistency, and the non-applicable benchmark gate
  passed in CI run `30818509376` at commit `ef7d54f`.
- Cairn architecture, decision, and interface verification passed in run
  `30818509424` at the same commit.
- Test harness correction: commit `b0f60b4` failed CI run `30819808916` with an
  `E0521` lifetime error because the new stdio helper borrowed a tool name into an
  owned MCP request field. Commit `f27142c` corrected the harness before the
  production red boundary was evaluated.
- Red: commit `f27142c` failed CI run `30820011444` with eight `E0308` type errors
  because `memory_search` and `memory_list` still accepted `SqliteStorage` while
  the parity tests required `LocalMemoryRuntime`.
- Boundary correction: CI run `30821520552` at commit `7b14d5a` rejected a
  crate-private tag lookup as dead library code. MCP is a separate binary crate,
  so the required runtime capability is intentionally public; Cairn scan recorded
  the resulting entrypoint interface hash as `98d8be9ec565753d`.
- Green: full Rust tests, Rustfmt, Clippy, smoke, npm installation, Python wrappers,
  installer integrity, version consistency, and the non-applicable benchmark gate
  passed in CI run `30821884066` at commit `377f906`.
- Cairn architecture, decision, and interface verification passed in run
  `30821884709` at the same commit.
- Red: commit `941a78f` failed CI run `31085615321` with sixteen `E0308`
  mismatches because the remaining five manage handlers still accepted
  `SqliteStorage` while the parity tests required the server-owned
  `LocalMemoryRuntime`.
- Implementation: commit `c55b8d6` routes the final manage family through the
  runtime, exposes only the two missing relationship-add and automatic-compaction
  delegates, consolidates unified execution onto the legacy handlers, and removes
  the duplicate server storage field. Exact-head verification follows on the
  Cairn evidence commit.
