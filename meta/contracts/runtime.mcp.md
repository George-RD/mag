---
node: mag.runtime.mcp
---
# mag.runtime.mcp contract

MCP handlers validate protocol inputs and delegate to domain/backend
capabilities. They do not reimplement memory semantics. Tool schemas and errors
remain stable, bounded, and safe for stdio transport.

The current production server is stdio-only and is constructed directly over
`SqliteStorage`; it does not pass through `memory_core::Pipeline`, substrate, an
LLM backend, or an HTTP daemon. Any additional transport must preserve the same
handler semantics and earn its own end-to-end lifecycle and authentication tests.
