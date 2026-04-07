# Forge-PR Stronghold

**PR:** George-RD/mag#248
**Branch:** fix/uninstall-binary-detection
**Base:** main
**Started:** 2026-04-07

## Phase 1: The Gathering

Status: done

Changes in working copy: `src/uninstall.rs` — fix `mag uninstall` binary detection for cargo/homebrew/pip/uv/npm/custom installs.

Key changes:
- `current_exe_path()` — running binary path, no canonicalize, strips ` (deleted)` suffix
- `install_method_hint()` — advisory hints for all install methods from path pattern
- `binary_install_label()` now reflects actual binary location (not hardcoded `~/.mag/bin`)
- `remove_binary_and_path()` → `remove_binary_and_path_impl(exe)` — injectable for tests
- `cargo_bin` check: `symlink_metadata().is_ok()` (handles broken symlinks)
- `BinaryResult`: `cargo_hint` + `other_binary` → `install_hints: Vec<String>`
- All 23 uninstall tests pass

Unintended files to exclude: `../README.md`, `../docs/strongholds/siege-pr239.md`

## Phase 2: The Forging

Status: done — jj initialized in mag/, unintended files excluded, fmt fixed, prek manual pass (fmt/clippy/tests all PASS), commit 77463367 on fix/uninstall-binary-detection

## Phase 3: The Summoning

Status: done — PR #248 open at https://github.com/George-RD/mag/pull/248

## Phase 4: The Watchers' Gaze

Status: done — all CRITICAL/MAJOR watcher findings fixed, 39 tests pass

## Phase 5: The Siege

Status: pending

## Phase 6: The Conquest

Status: pending
