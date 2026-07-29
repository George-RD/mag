---
node: mag.integrations.python
status: done
created: 2026-07-29
completed: 2026-07-29
priority: high
tags: [python, packaging, release, portability]
---
# Verify Python wrapper release provenance

The PyPI wrapper duplicated the release version in runtime code, so it could
install a different Rust binary from the version declared by the package. The
installed distribution metadata is now authoritative for both the public wrapper
version and the first-run binary download.

## Acceptance criteria

- [x] `mag_memory.__version__` reflects the installed `mag-memory` distribution.
- [x] First-run download passes that same version to the release downloader.
- [x] No manually maintained binary-version constant remains in the wrapper.
- [x] Python 3.8 and 3.13 exercise the wrapper path in pull-request CI.
- [x] The failing regression was observed before the implementation was added.

## Verification

- Red: CI run `30452205256` failed on Python 3.8 and 3.13 with the expected
  `0.1.10.dev0 != 0.1.5` drift, missing `_binary_version`, and duplicated
  `_BINARY_VERSION` failures.
- Green: CI run `30452493645` passed the new wrapper suite on Python 3.8 and
  Python 3.13 after the implementation.
- Cairn 0.9 regenerated the Python integration interface hash, reconciler cache,
  and map from the verified branch source.
