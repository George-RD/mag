---
node: mag.integrations.packaging
status: in_progress
created: 2026-07-29
priority: high
tags: [installer, packaging, integrity, portability]
---
# Enforce release checksums in the shell installer

The POSIX installer currently treats release checksum verification as optional.
It continues when no hashing utility is available, the checksum manifest cannot
be downloaded, or the target archive has no manifest entry. Align this path with
the fail-closed Python installer so every native release install has the same
integrity boundary.

## Acceptance criteria

- A supported SHA-256 utility is mandatory and missing tooling produces an
  actionable error.
- Failure to download `checksums.txt` aborts installation.
- The exact target archive must have one valid SHA-256 entry.
- Missing, malformed, duplicate, or mismatched entries abort installation.
- Substring filename collisions cannot select the wrong digest.
- A matching checksum continues through the existing installation path.
- The regression suite runs in pull-request CI.
- The failing regression is observed before implementation.
