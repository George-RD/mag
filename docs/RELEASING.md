# Releasing MAG

This document covers the full release process and rollback procedures for all five distribution registries.

## Release Procedure

### 1. Bump versions

```bash
./scripts/bump-version.sh <version> --commit
# Example: ./scripts/bump-version.sh 0.1.6 --commit
```

This atomically updates `Cargo.toml`, `npm/package.json`, and `python/pyproject.toml`, then commits.

### 2. Run quality gates

```bash
prek run    # fmt + clippy + tests
```

All gates must pass before proceeding.

### 3. Push and create PR

```bash
jj bookmark set release/vX.Y.Z -r @-
jj git push --bookmark release/vX.Y.Z --allow-new
gh pr create --head release/vX.Y.Z --base main \
  --title "chore: release vX.Y.Z" \
  --body "Release checklist: ..."
```

Or use the `/release` skill in Claude Code, which automates steps 1-4.

### 4. Tag and trigger release workflow

After the PR merges, create the release tag on `main`:

```bash
gh release create vX.Y.Z --title "MAG vX.Y.Z" --generate-notes
```

This triggers the GitHub Actions release workflow, which publishes to all registries automatically.

### 5. Verify packages are live

Check each registry within ~30 minutes of the workflow completing:

| Registry | Verification command |
| -------- | ------------------- |
| crates.io | `cargo install mag-memory@X.Y.Z --dry-run` |
| npm | `npm info mag-memory@X.Y.Z` |
| PyPI | `pip index versions mag-memory` |
| Homebrew | `brew info george-rd/mag/mag` |
| GitHub Releases | Check [Releases page](https://github.com/George-RD/mag/releases) |

---

## When to Rollback

Yank a release if it has any of the following:

- **Data loss** — stores, overwrites, or deletes user memories incorrectly.
- **Security vulnerability** — any issue in scope per [SECURITY.md](../SECURITY.md) (injection, auth bypass, data leakage).
- **Crash on startup** — the binary panics before accepting any MCP connections under normal conditions.
- **Silent corruption** — embeddings or search results are silently wrong, causing degraded retrieval with no error.

For bugs that are inconvenient but don't meet the above criteria, prefer a fast-follow patch release over a yank.

---

## Rollback Procedures

### crates.io

Yanking prevents new installs but does not affect existing installs.

```bash
cargo yank --version X.Y.Z mag-memory
```

To un-yank if the issue is resolved:

```bash
cargo yank --undo --version X.Y.Z mag-memory
```

### npm

Within 72 hours of publish, you can unpublish entirely. After 72 hours, use `deprecate`:

```bash
# Deprecate (always available)
npm deprecate mag-memory@X.Y.Z "known issue — use X.Y.(Z-1) instead"

# Unpublish within 72h window
npm unpublish mag-memory@X.Y.Z
```

### PyPI

PyPI does not have a CLI yank. Use the web UI:

1. Go to [https://pypi.org/manage/project/mag-memory/releases/](https://pypi.org/manage/project/mag-memory/releases/)
2. Select the version to yank.
3. Click **Yank** and add a reason.

A yanked version is hidden from `pip install mag-memory` (latest resolution) but still installable with a pinned version (`pip install mag-memory==X.Y.Z`).

### Homebrew

Revert the formula commit in the `homebrew-mag` tap:

```bash
# In the george-rd/homebrew-mag repository
git revert <commit-that-bumped-to-X.Y.Z>
git push
```

Users who have already upgraded will need to downgrade manually:

```bash
brew install george-rd/mag/mag@X.Y.(Z-1)
```

### GitHub Release

For minor issues (bad release notes, wrong asset): edit the release and mark it as **pre-release** to hide it from the "latest" tag.

For critical issues: delete the release and the tag so the release workflow can be re-triggered cleanly after a fix.

```bash
gh release delete vX.Y.Z --yes
gh api -X DELETE repos/George-RD/mag/git/refs/tags/vX.Y.Z
```

> Note: Deleting the GitHub release does **not** yank packages already published to crates.io, npm, or PyPI — those must be handled separately per the steps above.
