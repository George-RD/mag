---
node: mag.integrations.python
status: in_progress
created: 2026-07-29
priority: high
tags: [python, packaging, integrity, portability]
---
# Verify Python release checksums before extraction

The PyPI wrapper downloads a native release archive and extracts it without
checking the release checksum. Make checksum verification mandatory so a
corrupt, incomplete, or substituted archive cannot be installed.

## Acceptance criteria

- The wrapper fetches the release `checksums.txt` for the same version.
- The exact target archive must have one valid SHA-256 entry.
- A missing, malformed, duplicate, or mismatched checksum fails visibly.
- Verification completes before destination creation or archive extraction.
- Matching macOS/Linux tarballs and Windows zip archives continue through the
  existing extraction path.
- Python 3.8 and 3.13 run the integrity regression suite in pull-request CI.
- The failing regression is observed before implementation.
