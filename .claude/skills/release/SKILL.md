---
name: release
description: >-
  Release MAG to all distribution channels (GitHub Releases, crates.io, npm, PyPI).
  Use when user says "release", "publish", "cut a release", "ship it", "new version",
  or wants to push a new version to package registries.
tools: Read, Bash, Edit, Grep, Glob
---

# Release MAG

## Overview

MAG is distributed across 6 channels. The release CI workflow handles everything automatically on tag push. This skill guides the full release process.

| Channel | Package name | Install command |
|---------|-------------|-----------------|
| GitHub Releases | — | Download from releases page |
| crates.io | `mag-memory` | `cargo install mag-memory` |
| npm | `mag-memory` | `npm install -g mag-memory` |
| PyPI | `mag-memory` | `pip install mag-memory` |
| Homebrew | `mag` | `brew install George-RD/mag/mag` |

## Release flow

### Step 1: Determine version

Check current version in `Cargo.toml` and decide the next version (semver):
- **patch** (0.1.0 → 0.1.1): bug fixes
- **minor** (0.1.0 → 0.2.0): new features, backward compatible
- **major** (0.1.0 → 1.0.0): breaking changes

### Step 2: Bump versions in all files

Update the version string in ALL of these files (they must stay in sync):
1. `Cargo.toml` → `version = "X.Y.Z"`
2. `npm/package.json` → `"version": "X.Y.Z"`
3. `python/pyproject.toml` → `version = "X.Y.Z"`
4. `python/mag_memory/__init__.py` → `__version__` and `_BINARY_VERSION`
5. Homebrew formula version is auto-updated by CI (in `George-RD/homebrew-mag` repo)

### Step 3: Run quality gates

```bash
prek run
# Or manually:
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

### Step 4: Commit version bump

Commit the version bump changes with message: `chore: bump version to X.Y.Z`

### Step 5: Test with a release candidate

Push an RC tag first to test the CI pipeline without publishing to registries:

```bash
jj bookmark set vX.Y.Z-rc.1 -r @
jj git push --bookmark vX.Y.Z-rc.1
```

This triggers the full build matrix (5 targets) and creates a GitHub **prerelease**, but does NOT publish to crates.io/npm/PyPI.

Check the GitHub Actions run. If it fails, fix and push `-rc.2`, `-rc.3`, etc.

### Step 6: Ship the stable release

Once RC passes:

```bash
jj bookmark set vX.Y.Z -r @
jj git push --bookmark vX.Y.Z
```

This triggers:
1. Cross-compile for 5 targets (linux/macOS x86+arm, Windows)
2. Create GitHub Release with binaries + checksums
3. `cargo publish` to crates.io
4. `npm publish` to npm
5. Build + upload to PyPI (via Trusted Publishers OIDC)
6. Auto-update Homebrew tap (`George-RD/homebrew-mag`) with new checksums

### Step 7: Verify Homebrew tap update

The release CI automatically updates the `George-RD/homebrew-mag` tap repo with new version + SHA256 checksums. Check the `update-homebrew` job in GitHub Actions passed. If it fails, manually update `Formula/mag.rb` in the `George-RD/homebrew-mag` repo with checksums from the release's `checksums.txt`.

### Step 8: Verify

Check all channels received the release:
- GitHub: `https://github.com/George-RD/mag/releases`
- crates.io: `https://crates.io/crates/mag-memory`
- npm: `https://www.npmjs.com/package/mag-memory`
- PyPI: `https://pypi.org/project/mag-memory/`

## Tag conventions

| Tag pattern | Effect |
|-------------|--------|
| `vX.Y.Z-rc.N`, `vX.Y.Z-alpha.N`, `vX.Y.Z-beta.N` | Build + GitHub prerelease only. No registry publishing. |
| `vX.Y.Z` | Build + GitHub release + publish to all registries. |

## Troubleshooting

- **crates.io publish fails**: Check `CARGO_REGISTRY_TOKEN` secret is valid (expires every 90 days if scoped)
- **npm publish fails**: Check `NPM_TOKEN` secret is valid (90-day expiry). After first publish, switch to Trusted Publishing.
- **PyPI publish fails**: Check Trusted Publisher config at pypi.org/manage/account/publishing/ — must match repo `George-RD/mag`, workflow `release.yml`
- **Build fails on one target**: Check the matrix job logs. `aarch64-unknown-linux-gnu` uses `cross` and may need Docker. macOS/Windows are native builds.
- **Version already exists**: Registry versions are permanent. You must bump to a new version. You cannot re-publish the same version.

## Files involved

- `.github/workflows/release.yml` — the CI workflow
- `Cargo.toml` — Rust crate metadata + version
- `npm/package.json` — npm package metadata + version
- `npm/install.js` — npm postinstall binary downloader
- `python/pyproject.toml` — PyPI package metadata + version
- `python/mag_memory/__init__.py` — Python entry point + version
- `python/mag_memory/_download.py` — PyPI binary downloader
- `install.sh` — Shell installer (reads version from GitHub API)
- Homebrew formula lives in separate repo: `George-RD/homebrew-mag` (auto-updated by CI)
