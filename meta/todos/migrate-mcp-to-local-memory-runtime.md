---
node: mag.runtime.mcp
status: blocked
created: 2026-07-29
---
# Migrate MCP to the local memory runtime

Blocked by `todo.introduce-local-memory-runtime-facade` and the relevant stable
runtime capability slices.

Route both full and minimal MCP tool modes through the same transport-independent
runtime used by CLI. Start with failing protocol-level parity tests for tool
results, validation errors, and advertised tool sets. Preserve stdout exclusively
for MCP protocol traffic and keep diagnostics on stderr.

The stdio server remains the mandatory local baseline. This migration must not
introduce an HTTP, daemon, hosted-service, authentication, or cloud dependency.
Any future service adapter consumes the same runtime contract in a separate
milestone.
