---
node: mag.runtime.setup
status: done
created: 2026-07-29
completed: 2026-07-29
---
# Correct setup transport surface

`mag setup` now exposes only the transport that the binary can execute end to
end. Command mode writes `mag serve`; `http` and `stdio` fail before detection,
connector installation, model work, or tool configuration. The obsolete setup
port and daemon-check flags are gone, and HTTP remains a separate optional
service milestone rather than a dependency of local stdio operation.

## Acceptance criteria

- [x] Tests validate the generated command against the current CLI surface.
- [x] Unsupported transports fail before any tool configuration is modified.
- [x] Existing valid command transport remains idempotent and uninstall-symmetric.
- [x] User-facing setup and CLI text state which transport is actually available.
- [x] HTTP remains a separate optional service milestone rather than a local
  runtime dependency.
- [x] The regression was observed before implementation.

## Verification

- Red: CI run `30472000458` passed 646 existing tests and failed exactly the
  three new transport regressions. The failures proved `http` and `stdio` were
  accepted and direct setup orchestration modified the config instead of
  rejecting the request.
- Green: CI run `30472846392` passed all-feature Rust tests, formatting, Clippy,
  smoke coverage, npm installation, Python 3.8/3.13 wrappers, version checks,
  and shell-installer integrity with `sha256sum`, `shasum`, and `openssl`.
- Cairn: pinned 0.9 architecture gate run `30472854996` passed after the
  intentional setup interface baseline was refreshed.
