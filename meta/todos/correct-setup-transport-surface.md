---
node: mag.runtime.setup
status: open
created: 2026-07-29
---
# Correct setup transport surface

Setup currently advertises three transport modes, but only command transport is
executable end to end:

- `command` writes `mag serve` and reaches the stdio MCP server;
- `stdio` writes `mag serve --stdio`, but `serve` has no `--stdio` argument;
- `http` writes an HTTP URL, but no HTTP MCP server is assembled.

## Acceptance criteria

- Tests parse or otherwise validate every generated command against the current
  CLI surface.
- Unsupported transports fail before any tool configuration is modified.
- Existing valid command transport remains idempotent and uninstall-symmetric.
- User-facing setup and CLI text state which transport is actually available.
- HTTP remains a separate optional service milestone rather than a local runtime
  dependency.
- The regression is observed before implementation.
