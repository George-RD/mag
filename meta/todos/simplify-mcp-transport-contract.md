---
node: mag.runtime.mcp
status: in_progress
created: 2026-08-07
---
# Simplify MCP into an optional CLI-aligned transport

MAG's CLI is the canonical operating surface. MCP exists only where a host needs
MCP transport, and must adapt requests to the same application/runtime behavior
rather than owning a parallel memory application.

PR #414 closes the first architectural gap exposed by the 2026-08-07 review:
the binary consumes the library module graph, the entrypoint configures storage
and models once, and `McpMemoryServer` receives the same
`Arc<LocalMemoryRuntime>` used by CLI handlers. A pointer-identity regression
prevents MCP from silently constructing a second runtime again.

## Evidence

- 2026-08-08 — PR #412 upgraded the optional MCP transport from RMCP 0.16 to
  3.1.1 and migrated adapter/test construction to RMCP's typed constructors,
  content blocks, and response types. The reviewed source artifact from run
  `31265783135` (`sha256:68d85c874d32d6087cde256b82fa30c73e98944080b26e6fe324ecc37389e49b`)
  passed formatting, all-target/all-feature Clippy, all-feature tests, and the
  smoke suite. This is a transport-compatibility prerequisite; it does not
  complete the owned MCP contract-source work below.
- 2026-08-12 — PR #420 made `TOOL_REGISTRY` the owned source for full/minimal
  mode membership and counts, initialization instructions, protocol markdown,
  and CLI protocol JSON. A router-parity test now fails if RMCP `#[tool]`
  registration diverges from the registry. The contradictory 16-legacy wording
  was removed; the registry-derived contract is 4 facade tools plus 15 legacy
  compatibility tools. Exact-head `3b0a7d61ef291834d241e4b14745a4a6c915eee7`
  passed CI run `31619679372` and Cairn architecture run `31619679421` (the
  latter required one retry after GitHub returned HTTP 503 while downloading
  Cairn; the retry passed without source changes).

## Remaining work

- [x] Remove the binary-private production copies of `memory_core` and
  `local_memory_runtime`.
- [x] Construct one runtime after model and storage configuration, then inject it
  into the MCP adapter.
- [x] Keep MCP request adaptation and response formatting outside the runtime.
- [x] Define one owned MCP contract source from which tool advertisement,
  full/minimal modes, counts, initialization instructions, protocol output, and
  generated documentation are derived.
- [x] Remove the contradictory 15-versus-16 legacy-tool descriptions and add a
  contract parity test that makes future drift fail CI.
- [x] Prefer the four cohesive facade tools for new integrations. Retain legacy
  tools only through their explicit compatibility gate; do not expand them with
  new behavior.
- [ ] Align MAG skills and examples on CLI commands by default. Mention MCP only
  where the consuming AI host specifically requires MCP transport.
- [ ] Reassess whether full mode still earns its maintenance cost after the
  compatibility release. Any removal remains a separate public-contract change.

## Boundary

Do not make MCP shell out to the CLI in the current in-process stdio server. That
would add process lifecycle, JSON, and error-translation coupling while still
requiring the same application implementation. CLI remains the canonical
external contract; MCP calls the shared in-process runtime workflows and is
verified against CLI-visible behavior.

Do not fold MCP schemas or presentation into `LocalMemoryRuntime`. Deepen the
runtime only around cohesive memory workflows and typed outcomes that both CLI
and MCP need.

## Verification

- Exact-head Clippy must compile only the library-owned production module graph.
- Full and minimal stdio smoke/parity coverage must remain green.
- Generated MCP protocol documentation must be reproducible from the owned
  contract source.
- Cairn must show MCP depending on the entrypoint-owned runtime rather than
  owning a second composition root.