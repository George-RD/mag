---
name: release
description: >-
  Orchestrate a MAG release — bump, gate, PR, tag, verify — or rollback a broken release.
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
- **patch** (0.1.5 → 0.1.6): bug fixes
- **minor** (0.1.5 → 0.2.0): new features, backward compatible
- **major** (0.1.5 → 1.0.0): breaking changes

### Step 2: Bump versions

Run the atomic bump script — it updates all 3 manifests + Cargo.lock and commits:

```bash
./scripts/bump-version.sh vX.Y.Z --commit
```

This updates Cargo.toml, npm/package.json, python/pyproject.toml, regenerates Cargo.lock, and commits with `chore: bump version to vX.Y.Z`. Uses jj if available, git otherwise.

### Step 3: Quality gates

```bash
prek run                    # fmt + clippy + tests
./scripts/smoke-test.sh     # MCP server smoke test
./scripts/bench.sh --gate   # benchmark gate (if scoring/search changed)
```

All must pass before proceeding. If benchmark gate warns (>2pp delta), run full validation:
```bash
./scripts/bench.sh --samples 10 --notes "pre-release vX.Y.Z"
```

### Step 4: Push and create PR

```bash
jj bookmark set release/vX.Y.Z -r @-
jj git push --bookmark release/vX.Y.Z --allow-new
gh pr create --head release/vX.Y.Z --base main \
  --title "chore: release vX.Y.Z" \
  --body "## Release vX.Y.Z

### Gate results
- [ ] prek run: PASS
- [ ] smoke-test.sh: PASS
- [ ] bench.sh --gate: PASS (or N/A)

### Changes since vPREV
$(jj log -r 'main..@-' --no-graph --template 'description' | head -20)
"
```

### Step 5: Merge

Wait for CI to pass and review. Merge the PR when green.

### Step 6: Tag and trigger release

After merge, create the release tag:

```bash
# Via GitHub API (preferred — triggers release workflow):
TAG_SHA=$(gh api repos/George-RD/mag/git/ref/heads/main --jq '.object.sha')
gh api repos/George-RD/mag/git/refs -f ref="refs/tags/vX.Y.Z" -f sha="$TAG_SHA"
```

Or use workflow_dispatch if the release workflow supports it:
```bash
gh workflow run release.yml --repo George-RD/mag -f tag=vX.Y.Z
```

For release candidates (test CI pipeline without publishing):
```bash
jj bookmark set vX.Y.Z-rc.1 -r @
jj git push --bookmark vX.Y.Z-rc.1
```

### Step 7: Monitor release

```bash
gh run list --repo George-RD/mag --workflow=release.yml --limit 1
gh run watch  # interactive monitoring
```

The release workflow runs:
1. Preflight — version consistency check
2. Test suite — cargo test --all-features
3. Cross-compile — 5 targets (linux/macOS x86+arm, Windows)
4. GitHub Release — binaries + checksums
5. Publish — crates.io, npm, PyPI
6. Homebrew — auto-update tap with new checksums
7. Smoke test — install from crates.io + version check

### Step 8: Verify

Check all channels received the release:

```bash
# crates.io
cargo install mag-memory --version X.Y.Z && mag --version

# npm
npm view mag-memory@X.Y.Z version

# PyPI
pip install mag-memory==X.Y.Z

# GitHub
gh release view vX.Y.Z --repo George-RD/mag

# Homebrew (may take a few minutes for tap update)
brew update && brew info George-RD/mag/mag
```

### Step 9: Post-release dev bump

After verifying the release, bump `main` to the next development version so in-progress work is clearly marked as pre-release:

```bash
./scripts/bump-version.sh vX.Y.(Z+1)-dev --commit
jj git push
```

Example: after releasing v0.1.6, bump to v0.1.7-dev.

If the repo requires a PR for main (branch protection), open a quick PR instead of pushing directly:

```bash
jj bookmark set chore/dev-bump-vX.Y.(Z+1) -r @-
jj git push --bookmark chore/dev-bump-vX.Y.(Z+1) --allow-new
gh pr create --head chore/dev-bump-vX.Y.(Z+1) --base main \
  --title "chore: bump to vX.Y.(Z+1)-dev" \
  --body "Post-release dev bump."
```

Merge immediately once CI passes.

## Tag conventions

| Tag pattern | Effect |
|-------------|--------|
| `vX.Y.Z-rc.N`, `vX.Y.Z-alpha.N`, `vX.Y.Z-beta.N` | Build + GitHub prerelease only. No registry publishing. |
| `vX.Y.Z` | Build + GitHub release + publish to all registries. |

---

# Rollback a broken release

## When to rollback

- **Immediate yank**: data loss, security vulnerability, crash on startup
- **Deprecate**: subtle bug affecting many users, silent data corruption
- **Do nothing**: cosmetic issues, minor bugs with workarounds

## Registry-specific rollback

### crates.io
```bash
cargo yank --version X.Y.Z mag-memory
```
Prevents new `cargo install`, existing installs unaffected. Cannot undo a yank.

### npm
```bash
# Within 72 hours — full unpublish:
npm unpublish mag-memory@X.Y.Z

# After 72 hours — deprecate only:
npm deprecate mag-memory@X.Y.Z "known issue, use X.Y.(Z-1)"
```

### PyPI
Yank via web UI: pypi.org → project `mag-memory` → releases → select version → yank.
Prevents `pip install`, existing installs unaffected.

### Homebrew
Revert the formula commit in the `George-RD/homebrew-mag` tap:
```bash
cd homebrew-mag
git revert HEAD  # reverts the auto-updated formula
git push
```

### GitHub Release
```bash
# Mark as pre-release (keeps assets but warns users):
gh release edit vX.Y.Z --repo George-RD/mag --prerelease

# Or delete entirely:
gh release delete vX.Y.Z --repo George-RD/mag --yes
gh api repos/George-RD/mag/git/refs/tags/vX.Y.Z -X DELETE
```

## Troubleshooting

- **crates.io publish fails**: Check `CARGO_REGISTRY_TOKEN` secret is valid (expires every 90 days if scoped)
- **npm publish fails**: Check `NPM_TOKEN` secret is valid (90-day expiry)
- **PyPI publish fails**: Check Trusted Publisher config at pypi.org — must match repo `George-RD/mag`, workflow `release.yml`
- **Build fails on one target**: Check matrix job logs. `aarch64-unknown-linux-gnu` uses `cross` and may need Docker.
- **Version already exists**: Registry versions are permanent. Bump to a new version.
- **bump-version.sh fails**: Requires `jq`. Run `command -v jq` to check. Install via `brew install jq` or `apt install jq`.

## Files involved

- `.github/workflows/release.yml` — the CI workflow
- `scripts/bump-version.sh` — atomic version bumper
- `scripts/smoke-test.sh` — MCP server smoke test
- `Cargo.toml` — Rust crate metadata + version
- `npm/package.json` — npm package metadata + version
- `python/pyproject.toml` — PyPI package metadata + version
- `docs/RELEASING.md` — detailed release + rollback runbook
- Homebrew formula lives in separate repo: `George-RD/homebrew-mag` (auto-updated by CI)
