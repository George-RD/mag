---
node: mag.runtime.entrypoints
---
# mag.runtime.entrypoints contract

The entrypoint layer owns process startup, CLI dispatch, and assembly of concrete
components. It must not contain independent storage, retrieval, extraction, or
connector semantics. MCP mode keeps stdout exclusively for protocol traffic and
sends diagnostics to stderr.

The current binary constructs the production embedder and `SqliteStorage`.
`memory_core::Pipeline` is used only as a compatibility-sensitive adapter for a
subset of CLI commands; MCP and most extended CLI operations delegate directly
to SQLite capabilities. The entrypoint does not currently construct substrate,
an LLM backend, or an HTTP server. New production intelligence must enter through
the selected composition root rather than another command-specific path.
