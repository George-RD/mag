#!/usr/bin/env bash
# bump-version.sh — Atomically bump version across all 3 manifests:
#   Cargo.toml, npm/package.json, python/pyproject.toml
#
# Usage:
#   ./scripts/bump-version.sh <version>           # bump and show diff
#   ./scripts/bump-version.sh <version> --commit  # bump, show diff, and commit
#   ./scripts/bump-version.sh --help              # show this message
#
# Examples:
#   ./scripts/bump-version.sh 0.1.6
#   ./scripts/bump-version.sh v0.1.6 --commit

set -euo pipefail

# ── helpers ──────────────────────────────────────────────────────────────────

usage() {
  grep '^#' "$0" | sed 's/^# \{0,1\}//' | tail -n +2
  exit 0
}

die() {
  echo "error: $*" >&2
  exit 1
}

# ── arg parsing ───────────────────────────────────────────────────────────────

VERSION=""
DO_COMMIT=false

for arg in "$@"; do
  case "$arg" in
    --help|-h)   usage ;;
    --commit)    DO_COMMIT=true ;;
    -*)          die "unknown flag: $arg" ;;
    *)
      if [[ -n "$VERSION" ]]; then
        die "unexpected argument '$arg' — version already set to '$VERSION'"
      fi
      VERSION="$arg"
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version> [--commit]" >&2
  echo "Run '$0 --help' for full usage." >&2
  exit 1
fi

# Strip leading 'v' if present
VERSION="${VERSION#v}"

# Validate semver X.Y.Z or X.Y.Z-prerelease (e.g. 0.1.7-dev, 0.1.6-rc.1)
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
  die "invalid version '$VERSION' — expected X.Y.Z or X.Y.Z-suffix (e.g. 0.1.6, 0.1.7-dev)"
fi

# ── require jq ───────────────────────────────────────────────────────────────

command -v jq >/dev/null 2>&1 || die "jq is required (brew install jq / apt install jq)"

echo "Bumping version to $VERSION ..."

# ── resolve repo root ─────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"

CARGO_TOML="$REPO_ROOT/Cargo.toml"
PKG_JSON="$REPO_ROOT/npm/package.json"
PYPROJECT="$REPO_ROOT/python/pyproject.toml"

# ── validate files exist ──────────────────────────────────────────────────────

for f in "$CARGO_TOML" "$PKG_JSON" "$PYPROJECT"; do
  [[ -f "$f" ]] || die "file not found: $f"
done

# ── backup & rollback on failure ──────────────────────────────────────────────

cp "$CARGO_TOML"         "$CARGO_TOML.bak"
cp "$PKG_JSON"           "$PKG_JSON.bak"
cp "$PYPROJECT"          "$PYPROJECT.bak"
cp "$REPO_ROOT/Cargo.lock" "$REPO_ROOT/Cargo.lock.bak" 2>/dev/null || true

SUCCESS=false

restore_backups() {
  echo "error: restoring original files due to failure" >&2
  cp "$CARGO_TOML.bak" "$CARGO_TOML"
  cp "$PKG_JSON.bak"   "$PKG_JSON"
  cp "$PYPROJECT.bak"  "$PYPROJECT"
  [[ -f "$REPO_ROOT/Cargo.lock.bak" ]] && cp "$REPO_ROOT/Cargo.lock.bak" "$REPO_ROOT/Cargo.lock"
}

remove_backups() {
  rm -f "$CARGO_TOML.bak" "$PKG_JSON.bak" "$PYPROJECT.bak" "$REPO_ROOT/Cargo.lock.bak"
}

trap 'if ! $SUCCESS; then restore_backups; fi; remove_backups' EXIT

# ── update Cargo.toml ─────────────────────────────────────────────────────────
# Update the FIRST occurrence of `version = "..."` (the [package] version,
# not a dependency version).

OLD_CARGO=$(grep -m1 '^version = "' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ -n "$OLD_CARGO" ]] || die "could not find version in $CARGO_TOML"

# Use awk to only replace the first matching line
awk -v new="$VERSION" '
  !done && /^version = "/ {
    sub(/"[^"]*"/, "\"" new "\"")
    done=1
  }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"

ACTUAL=$(grep -m1 '^version = "' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ "$ACTUAL" == "$VERSION" ]] || die "Cargo.toml update failed (got '$ACTUAL')"
echo "  Cargo.toml:            $OLD_CARGO → $VERSION"

# ── update Cargo.lock ─────────────────────────────────────────────────────────

if command -v cargo >/dev/null 2>&1; then
  if ! cargo generate-lockfile --manifest-path "$CARGO_TOML" 2>/dev/null; then
    if ! cargo check --manifest-path "$CARGO_TOML" 2>/dev/null; then
      echo "Warning: could not regenerate Cargo.lock (cargo not available)" >&2
    fi
  fi
  echo "  Cargo.lock:            regenerated"
fi

# ── update npm/package.json ───────────────────────────────────────────────────

OLD_NPM=$(jq -r '.version' "$PKG_JSON")
[[ -n "$OLD_NPM" && "$OLD_NPM" != "null" ]] || die "could not find version in $PKG_JSON"

jq --arg v "$VERSION" '.version = $v' "$PKG_JSON" > "$PKG_JSON.tmp" && mv "$PKG_JSON.tmp" "$PKG_JSON"

ACTUAL=$(jq -r '.version' "$PKG_JSON")
[[ "$ACTUAL" == "$VERSION" ]] || die "npm/package.json update failed (got '$ACTUAL')"
echo "  npm/package.json:      $OLD_NPM → $VERSION"

# ── update python/pyproject.toml ─────────────────────────────────────────────
# Only update version under [project], not [build-system] or other sections.

OLD_PY=$(awk '/^\[project\]/{in_proj=1} /^\[/{if(!/^\[project\]/)in_proj=0} in_proj && /^version = "/{gsub(/.*version = "|".*/,""); print; exit}' "$PYPROJECT")
[[ -n "$OLD_PY" ]] || die "could not find version in $PYPROJECT"

awk -v new="$VERSION" '
  /^\[project\]/ { in_proj=1 }
  /^\[/ { if (!/^\[project\]/) in_proj=0 }
  in_proj && !done && /^version = "/ {
    sub(/"[^"]*"/, "\"" new "\"")
    done=1
  }
  { print }
' "$PYPROJECT" > "$PYPROJECT.tmp" && mv "$PYPROJECT.tmp" "$PYPROJECT"

ACTUAL=$(awk '/^\[project\]/{in_proj=1} /^\[/{if(!/^\[project\]/)in_proj=0} in_proj && /^version = "/{gsub(/.*version = "|".*/,""); print; exit}' "$PYPROJECT")
[[ "$ACTUAL" == "$VERSION" ]] || die "python/pyproject.toml update failed (got '$ACTUAL')"
echo "  python/pyproject.toml: $OLD_PY → $VERSION"

# ── all updates succeeded — EXIT trap will clean up backups ──────────────────

# ── show diff ────────────────────────────────────────────────────────────────

echo ""
echo "Diff:"
git -C "$REPO_ROOT" diff -- Cargo.toml Cargo.lock npm/package.json python/pyproject.toml

# ── optional commit ───────────────────────────────────────────────────────────

if [[ "$DO_COMMIT" == true ]]; then
  echo ""
  git -C "$REPO_ROOT" add Cargo.toml Cargo.lock npm/package.json python/pyproject.toml

  if command -v jj >/dev/null 2>&1 && [[ -d "$REPO_ROOT/.jj" ]]; then
    jj --repository "$REPO_ROOT" describe -m "chore: bump version to v$VERSION"
    jj --repository "$REPO_ROOT" new
  else
    git -C "$REPO_ROOT" commit -m "chore: bump version to v$VERSION"
  fi

  echo "Committed: chore: bump version to v$VERSION"
fi

SUCCESS=true
