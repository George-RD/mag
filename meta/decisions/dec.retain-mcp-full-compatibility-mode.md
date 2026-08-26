---
id: dec.retain-mcp-full-compatibility-mode
nodes:
  - mag.runtime.mcp
  - mag.runtime.entrypoints
status: accepted
date: 2026-08-25
revisit_triggers:
  - "A versioned MCP contract release is prepared with an explicit migration path for legacy tool callers"
  - "Usage evidence shows the legacy compatibility tools no longer have meaningful callers"
  - "Maintaining the legacy tool schemas requires independent semantics or materially duplicates application workflows"
  - "A supported MCP host cannot use the four facade tools without legacy advertisement"
informed_by:
  - dec.select-local-runtime-composition-root
---
# Retain full MCP mode as compatibility while new integrations prefer minimal

## Decision

MAG keeps the full 19-tool MCP mode as an explicit compatibility surface for the
current release line. `mag serve` therefore keeps its existing full-mode default,
so existing manual configurations that launch plain `mag serve` do not silently
lose legacy tool names after an upgrade.

New MCP integrations created by `mag setup`, and new manual setup examples, shall
explicitly request `--mcp-tools minimal`. Minimal mode advertises the four unified
facades and is the preferred MCP contract for new callers. Full mode remains
available through `--mcp-tools full` and the existing default until a separate,
versioned public-contract decision removes or changes it.

## Boundary rules

1. The CLI remains MAG's canonical external interface. MCP is optional transport.
2. New MCP integrations prefer the four facade tools; no new product behavior is
   added only to the 15 legacy compatibility tools.
3. Legacy and facade tools must delegate to the same typed application/runtime
   workflows. Retaining full mode is not permission to retain duplicate storage,
   retrieval, lifecycle, or orchestration logic.
4. Existing detected MCP entries are not automatically rewritten merely because
   they launch plain `mag serve`. The normal setup flow treats an existing MAG
   entry as configured; an explicit reconfiguration may generate the preferred
   minimal contract.
5. Removing full mode, changing the default for existing manual callers, or
   deleting legacy tool names is a public-contract change and requires its own
   versioned migration decision and regression coverage.

## Evidence

PR #425 repaired the four-tool minimal surface so `memory(action="search")`
delegates to the same typed search adapter/runtime workflow as the compatibility
`memory_search` tool. Exact-head `c736ee4a141749cc9c4a5e2e469f3893faf7e21e`
passed CI run `32833962920` and Cairn architecture run `32833962924` before merge.
Minimal mode can therefore perform MAG's core store, retrieve, search, management,
session, and administration workflows without advertising legacy tool names.

The follow-up setup audit found that `mag setup` still generated command entries
with only `args: ["serve"]`. Because `mag serve` intentionally defaults to full
mode for compatibility, those newly generated integrations advertised all 19
tools and contradicted the preferred new-integration contract. PR #426 pins this
with a red exact-head regression at
`aa1bfeadd00f7e394432e876a39bddee83284a14`: CI run `32834662260` failed only the
new generated-config contract test after the existing Rust suites passed, while
Cairn architecture run `32834662273` remained green.

Implementation commit `b7b85e952779e93695748db9149ba97dd2804bbe`
updates command-mode JSON and TOML generation plus the bundled Claude plugin
manifest to request `--mcp-tools minimal`; full-mode and stdio behavior are left
unchanged. Focused run `32950589249` passed the contract and config-writer tests.
Broader CI then exposed two setup tests that still encoded the old generated
invocation. Commit `bf435de85546a29a644630c0594d4361b1371c01`
aligned those expectations, and focused run `32951209730` passed both regressions.

## Trade-offs

Keeping full mode means MAG continues to advertise and test 15 compatibility tool
schemas for the current release line. That maintenance cost is acceptable while
it prevents silent breakage for existing MCP callers. The cost is bounded by
making minimal mode the default for newly generated integrations and by refusing
to add independent behavior to legacy wrappers.

Changing `mag serve` itself to minimal now would be simpler internally, but would
silently change the tool inventory for existing configurations that rely on the
current default. Removing full mode now would create the same compatibility risk.
Both are rejected until a versioned migration has evidence that legacy callers
can be retired safely.
