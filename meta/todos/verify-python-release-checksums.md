---
node: mag.integrations.python
status: done
created: 2026-07-29
completed: 2026-07-29
priority: high
tags: [python, packaging, integrity, portability]
---
# Verify Python release checksums before extraction

The PyPI wrapper now treats the version-matched release checksum manifest as a
mandatory install prerequisite. It verifies the exact target archive before
creating the destination directory or invoking either extraction path.

## Acceptance criteria

- [x] The wrapper fetches the release `checksums.txt` for the same version.
- [x] The exact target archive must have one valid SHA-256 entry.
- [x] A missing, malformed, duplicate, or mismatched checksum fails visibly.
- [x] Verification completes before destination creation or archive extraction.
- [x] Matching macOS/Linux tarballs and Windows zip archives continue through the
  existing extraction path.
- [x] Python 3.8 and 3.13 run the integrity regression suite in pull-request CI.
- [x] The failing regression was observed before implementation.

## Verification

- Red: CI run `30453341364` failed on Python 3.8 and 3.13 with six missing
  checksum API errors, a mismatch that raised nothing, and manifest bytes flowing
  directly into extraction.
- Green: CI run `30453499823` passed the full wrapper suite on Python 3.8 and
  Python 3.13 after checksum verification was implemented.
- Cairn 0.9 regenerated the Python integration interface hash, reconciler cache,
  and map from the branch source.
