---
node: mag.runtime.daemon
---
# mag.runtime.daemon contract

The daemon node currently owns feature-gated support primitives: daemon metadata,
Bearer-token middleware, and idle-lifecycle middleware. It does not currently
assemble an HTTP router, listener, MCP transport, background process, or metadata
writer, so it must not be presented as a working deployment adapter.

Any future service mode must reuse the same domain behaviour as local CLI/MCP,
keep authentication and lifecycle at the transport boundary, provide explicit
health and shutdown semantics, and remain optional. Local operation must never
depend on an HTTP daemon, hosted service, or cloud credential.
