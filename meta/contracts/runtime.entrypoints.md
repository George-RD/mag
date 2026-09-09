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

## Non-persisting intelligence evaluation

With the `llm` feature, `mag intelligence-produce` is a thin stdio/configuration
adapter over `LocalMemoryRuntime::produce_intelligence`. It dispatches before
storage or embedding initialization. One strict, answer-blind protocol-v1
request carries only task, instruction, and immutable source IDs/text; unknown
and duplicate JSON fields, invalid versions/tasks, blank inputs, duplicate
source IDs, and oversized requests fail before inference.

The runtime owns validation and one completion attempt. Plain mode remains the
default; `--json-schema` selects `produce_intelligence_with_schema`, adding only
the fixed native output constraint. `--describe` records the requested mode and
exact schema without claiming server enforcement. It does not
persist, repair, parse the output schema, retry, or supply expected annotations.
The independent evaluation scorer owns output validity and quality judgment.
`--describe` reads neither stdin nor a model and labels its settings
`configured_not_authenticated`, with absent artifact and embedding identity
fields explicitly null. This opt-in workflow does not add ingestion-time
memory generation or a second MCP implementation.
