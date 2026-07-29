---
node: mag.runtime.entrypoints
---
# mag.runtime.entrypoints contract

The entrypoint layer owns process startup, CLI dispatch, and assembly of concrete
components. It must not contain independent storage, retrieval, extraction, or
connector semantics. MCP mode keeps stdout exclusively for protocol traffic and
sends diagnostics to stderr.

The selected production composition root is one transport-independent local
memory runtime, constructed once by the process entrypoint and shared by CLI and
MCP. Its initial implementation wraps the selected embedder/model roles, one
`SqliteStorage`, and the current capability delegates without changing behaviour.

During migration, direct SQLite calls and `memory_core::Pipeline` construction are
tracked compatibility paths only. New production intelligence must enter through
the local runtime rather than a command-specific branch. The runtime must remain
usable by local stdio without a daemon, HTTP server, cloud credential, or network
service; optional service adapters depend on the runtime, not the reverse.
