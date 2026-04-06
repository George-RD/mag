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

# Validate semver X.Y.Z
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  die "invalid version '$VERSION' — expected X.Y.Z (e.g. 0.1.6)"
fi

# ── require jq ───────────────────────────────────────────────────────────────

command -v jq >/dev/null 2>&1 || die "jq is required (brew install jq / apt install jq)"

echo "Bumping version to $VERSION ..."

# ── resolve repo root ─────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"

CARGO_TOML="$REPO_ROOT/Cargo.toml"
CARGO_LOCK="$REPO_ROOT/Cargo.lock"
PKG_JSON="$REPO_ROOT/npm/package.json"
PYPROJECT="$REPO_ROOT/python/pyproject.toml"

# ── validate files exist ──────────────────────────────────────────────────────

for f in "$CARGO_TOML" "$PKG_JSON" "$PYPROJECT"; do
  [[ -f "$f" ]] || die "file not found: $f"
done

# ── backup & rollback on failure ──────────────────────────────────────────────

restore_backups() {
  echo "error: restoring original files due to failure" >&2
  [[ -f "$CARGO_TOML.bak" ]] && mv "$CARGO_TOML.bak" "$CARGO_TOML"
  [[ -f "$PKG_JSON.bak"   ]] && mv "$PKG_JSON.bak"   "$PKG_JSON"
  [[ -f "$PYPROJECT.bak"  ]] && mv "$PYPROJECT.bak"  "$PYPROJECT"
}

remove_backups() {
  rm -f "$CARGO_TOML.bak" "$PKG_JSON.bak" "$PYPROJECT.bak"
}

cp "$CARGO_TOML" "$CARGO_TOML.bak"
cp "$PKG_JSON"   "$PKG_JSON.bak"
cp "$PYPROJECT"  "$PYPROJECT.bak"

trap 'restore_backups' ERR

# ── update Cargo.toml ─────────────────────────────────────────────────────────
# Update the FIRST occurrence of `version = "..."` (the [package] version,
# not a dependency version).

OLD_CARGO=$(grep -m1 '^version = "' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
if [[ -z "$OLD_CARGO" ]]; then
  die "could not find version in $CARGO_TOML"
fi

# Use awk to only replace the first matching line
awk -v new="$VERSION" '
  !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+"/ {
    sub(/"[0-9]+\.[0-9]+\.[0-9]+"/, "\"" new "\"")
    done=1
  }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# Verify
ACTUAL=$(grep -m1 '^version = "' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ "$ACTUAL" == "$VERSION" ]] || die "Cargo.toml update failed (got '$ACTUAL')"
echo "  Cargo.toml:            $OLD_CARGO → $VERSION"

# ── update Cargo.lock ─────────────────────────────────────────────────────────

if command -v cargo >/dev/null 2>&1; then
  cargo generate-lockfile --manifest-path "$CARGO_TOML" 2>/dev/null \
    || cargo check --manifest-path "$CARGO_TOML" --quiet 2>/dev/null \
    || true
  echo "  Cargo.lock:            regenerated"
fi

# ── update npm/package.json ───────────────────────────────────────────────────

OLD_NPM=$(jq -r '.version' "$PKG_JSON")
if [[ -z "$OLD_NPM" || "$OLD_NPM" == "null" ]]; then
  die "could not find version in $PKG_JSON"
fi

jq --arg v "$VERSION" '.version = $v' "$PKG_JSON" > "$PKG_JSON.tmp" && mv "$PKG_JSON.tmp" "$PKG_JSON"

ACTUAL=$(jq -r '.version' "$PKG_JSON")
[[ "$ACTUAL" == "$VERSION" ]] || die "npm/package.json update failed (got '$ACTUAL')"
echo "  npm/package.json:      $OLD_NPM → $VERSION"

# ── update python/pyproject.toml ─────────────────────────────────────────────
# Only update version under [project], not [build-system] or other sections.

OLD_PY=$(awk '/^\[project\]/{in_proj=1} /^\[/{if(!/^\[project\]/)in_proj=0} in_proj && /^version = "/{gsub(/.*version = "|".*/,""); print; exit}' "$PYPROJECT")
if [[ -z "$OLD_PY" ]]; then
  die "could not find version in $PYPROJECT"
fi

awk -v new="$VERSION" '
  /^\[project\]/ { in_proj=1 }
  /^\[/ { if (!/^\[project\]/) in_proj=0 }
  in_proj && !done && /^version = "[0-9]+\.[0-9]+\.[0-9]+"/ {
    sub(/"[0-9]+\.[0-9]+\.[0-9]+"/, "\"" new "\"")
    done=1
  }
  { print }
' "$PYPROJECT" > "$PYPROJECT.tmp" && mv "$PYPROJECT.tmp" "$PYPROJECT"

ACTUAL=$(awk '/^\[project\]/{in_proj=1} /^\[/{if(!/^\[project\]/)in_proj=0} in_proj && /^version = "/{gsub(/.*version = "|".*/,""); print; exit}' "$PYPROJECT")
[[ "$ACTUAL" == "$VERSION" ]] || die "python/pyproject.toml update failed (got '$ACTUAL')"
echo "  python/pyproject.toml: $OLD_PY → $VERSION"

# ── all updates succeeded — remove backups ────────────────────────────────────

trap - ERR
remove_backups

# ── show diff ────────────────────────────────────────────────────────────────

echo ""
echo "Diff:"
git -C "$REPO_ROOT" diff -- Cargo.toml Cargo.lock npm/package.json python/pyproject.toml

# ── optional commit ───────────────────────────────────────────────────────────

if [[ "$DO_COMMIT" == true ]]; then
  echo ""
  COMMIT_MSG="chore: bump version to v$VERSION"

  if command -v jj >/dev/null 2>&1 && [[ -d "$REPO_ROOT/.jj" ]]; then
    # jj-colocated repo: stage files first (git add), then describe + new
    git -C "$REPO_ROOT" add Cargo.toml Cargo.lock npm/package.json python/pyproject.toml
    jj describe -m "$COMMIT_MSG"
    jj new
  else
    git -C "$REPO_ROOT" add Cargo.toml Cargo.lock npm/package.json python/pyproject.toml
    git -C "$REPO_ROOT" commit -m "$COMMIT_MSG"
  fi

  echo "Committed: $COMMIT_MSG"
fi
