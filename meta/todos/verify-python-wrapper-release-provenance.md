---
node: mag.integrations.python
status: in_progress
created: 2026-07-29
priority: high
tags: [python, packaging, release, portability]
---
# Verify Python wrapper release provenance

The PyPI wrapper currently duplicates the release version in runtime code, so it
can install a different Rust binary from the version declared by the package.
Remove that independent version source and make the installed distribution
metadata authoritative.

## Acceptance criteria

- `mag_memory.__version__` reflects the installed `mag-memory` distribution.
- First-run download passes that same version to the release downloader.
- No manually maintained binary-version constant remains in the wrapper.
- Python 3.8 and 3.13 exercise the wrapper path in pull-request CI.
- The failing regression is observed before the implementation is added.
