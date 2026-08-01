---
node: mag.runtime.mcp
status: in_progress
created: 2026-07-29
---
# Migrate MCP to the local memory runtime

The runtime facade and all CLI command families are complete. MCP migration is
now in progress by bounded tool family so each slice can preserve protocol behaviour
independently.

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
