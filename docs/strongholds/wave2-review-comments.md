# Wave 2 — Review Comments Stronghold

## Status: complete (PR #180)

Compiled from CodeRabbit + human reviews on PRs #174-#179 (2026-04-01).

---

## Actionable Items

### 1. PR #176 — `has_scope` guard bug (REAL BUG)
- **File**: `src/memory_core/storage/sqlite/admin.rs` (welcome_scoped impl)
- **Issue**: The `has_scope` guard omits `project`, so callers passing only `project=Some(...)` with no budget fall through to the 200-entry budgeted path instead of the lean `welcome()` fast-path.
- **Fix**: Opposite of what CodeRabbit suggested — `project` was already IN the guard, but should be REMOVED since `welcome()` already handles project filtering. Renamed guard to `has_extra_scope`.
- **Severity**: Medium — correctness bug, performance impact on project-only queries.
- **Status**: FIXED in PR #180

### 2. PR #176 — Tier order substring matching (BRITTLE)
- **File**: `src/memory_core/storage/sqlite/admin.rs`
- **Issue**: Tier order clause derived by substring matching on SQL strings — brittle and fragile.
- **Fix**: Already resolved — code uses explicit `Tier` struct with `order` field (not substring matching).
- **Severity**: Low — design smell, not a runtime bug.
- **Status**: NOT A BUG (already uses struct)

### 3. PR #176 — Misleading greedy-fit comment (COMMENT/CODE MISMATCH)
- **File**: `src/memory_core/storage/sqlite/admin.rs`
- **Issue**: Comment says "try smaller entries later" but code `break`s immediately when token budget is exceeded.
- **Fix**: Misleading comment already removed. `break` is correct: entries within a tier have constant `cap_chars`, so if one doesn't fit, later ones won't either.
- **Severity**: Low — was misleading docs, already resolved.
- **Status**: NOT A BUG (comment removed, break behavior is correct)

### 4. PR #179 — Test gap for `mcp_tools` default (BLOCKING PR MERGE)
- **File**: `src/cli.rs:1101-1103`
- **Issue**: Test uses `{ cross_encoder, .. }` hiding the new `mcp_tools` field — no assertion on default value.
- **Fix**: Explicitly destructure `mcp_tools` and add `assert_eq!`.
- **Severity**: Medium — missing test coverage for new feature flag.
- **Status**: FIXED in PR #179 (mcp_tools now explicitly destructured and asserted)

---

## PRs with NO CodeRabbit review (rate-limited)

- PR #174: WelcomeOptions struct + trait method — **zero automated review**
- PR #175: Unified memory facade tool — **zero automated review**

Manually reviewed during Gate 2 (/dg) in PR #180. Both clean — no issues found.

---

## Patterns

1. **CLI test `..` anti-pattern**: When new fields added to `Commands::Serve`, existing tests use `..` and lose default-value coverage.
2. **Comment/code drift**: Comments describing future behavior that was never implemented.
3. **Scope guard incompleteness**: New optional fields not added to guard conditions.
