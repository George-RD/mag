---
node: mag.runtime.setup
status: in_progress
created: 2026-07-29
---
# Correct setup transport surface

Setup currently advertises three transport modes, but only command transport is
executable end to end:

- `command` writes `mag serve` and reaches the stdio MCP server;
- `stdio` writes `mag serve --stdio`, but `serve` has no `--stdio` argument;
- `http` writes an HTTP URL, but no HTTP MCP server is assembled.

## TDD evidence

- Red was observed in CI run `30472000458`: 646 tests passed and the three new
  transport regressions failed because `http`, `stdio`, and direct setup writes
  were still accepted.
- The implementation now rejects unsupported modes at parsing and setup
  orchestration boundaries before detection or file access, retains the valid
  `mag serve` command entry, removes misleading daemon flags/checks, and updates
  setup help and maintained guidance.
- The first implementation run exposed a stale full-flow test fixture and the
  expected Cairn interface-hash change. Both are corrected; full repository and
  Cairn verification is rerunning on the corrected head.

## Acceptance criteria

- Tests parse or otherwise validate every generated command against the current
  CLI surface.
- Unsupported transports fail before any tool configuration is modified.
- Existing valid command transport remains idempotent and uninstall-symmetric.
- User-facing setup and CLI text state which transport is actually available.
- HTTP remains a separate optional service milestone rather than a local runtime
  dependency.
- The regression is observed before implementation.
