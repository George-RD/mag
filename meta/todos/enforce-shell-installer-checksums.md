---
node: mag.integrations.packaging
status: done
created: 2026-07-29
completed: 2026-07-29
priority: high
tags: [installer, packaging, integrity, portability]
---
# Enforce release checksums in the shell installer

The POSIX installer now treats the version-matched release checksum manifest as
a mandatory install prerequisite. It selects a supported native SHA-256 utility,
requires one exact archive entry, and verifies the archive before extraction or
installation continues.

## Acceptance criteria

- [x] A supported SHA-256 utility is mandatory and missing tooling produces an
  actionable error.
- [x] Failure to download `checksums.txt` aborts installation.
- [x] The exact target archive must have one valid SHA-256 entry.
- [x] Missing, malformed, duplicate, or mismatched entries abort installation.
- [x] Substring filename collisions cannot select the wrong digest.
- [x] A matching checksum continues through the existing installation path.
- [x] The regression suite runs in pull-request CI.
- [x] The failing regression was observed before implementation.

## Verification

- Red: CI run `30454600639` exposed six fail-open or ambiguous behaviours,
  including missing tooling, unavailable manifests, absent exact entries,
  malformed and duplicate entries, and filename collisions.
- Green: CI run `30455636105` passed eight installer integrity cases separately
  with `sha256sum`, macOS `shasum`, and `openssl`.
- Cairn 0.9 architecture gate run `30455636100` passed for the completed branch
  state.
