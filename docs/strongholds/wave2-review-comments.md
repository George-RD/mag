# Wave 2 — Review Comments Stronghold

## Status: in-progress

Compiled from CodeRabbit + human reviews on PRs #174-#179 (2026-04-01).

---

## Actionable Items

### 1. PR #176 — `has_scope` guard bug (REAL BUG)
- **File**: `src/memory_core/storage/sqlite/admin.rs` (welcome_scoped impl)
- **Issue**: The `has_scope` guard omits `project`, so callers passing only `project=Some(...)` with no budget fall through to the 200-entry budgeted path instead of the lean `welcome()` fast-path.
- **Fix**: Add `opts.project.is_some()` to the scope check.
- **Severity**: Medium — correctness bug, performance impact on project-only queries.
- **Status**: OPEN

### 2. PR #176 — Tier order substring matching (BRITTLE)
- **File**: `src/memory_core/storage/sqlite/admin.rs`
- **Issue**: Tier order clause derived by substring matching on SQL strings — brittle and fragile.
- **Fix**: Add an explicit `order_clause` field to the tier tuple.
- **Severity**: Low — design smell, not a runtime bug.
- **Status**: OPEN

### 3. PR #176 — Misleading greedy-fit comment (COMMENT/CODE MISMATCH)
- **File**: `src/memory_core/storage/sqlite/admin.rs`
- **Issue**: Comment says "try smaller entries later" but code `break`s immediately when token budget is exceeded.
- **Fix**: Either implement greedy-fit (`continue` instead of `break`) or fix the misleading comment.
- **Severity**: Low — misleading docs, not a runtime bug unless greedy-fit is desired.
- **Status**: OPEN

### 4. PR #179 — Test gap for `mcp_tools` default (BLOCKING PR MERGE)
- **File**: `src/cli.rs:1101-1103`
- **Issue**: Test uses `{ cross_encoder, .. }` hiding the new `mcp_tools` field — no assertion on default value.
- **Fix**: Explicitly destructure `mcp_tools` and add `assert_eq!`.
- **Severity**: Medium — missing test coverage for new feature flag.
- **Status**: Assigned to rustfmt-fixer agent

---

## PRs with NO CodeRabbit review (rate-limited)

- PR #174: WelcomeOptions struct + trait method — **zero automated review**
- PR #175: Unified memory facade tool — **zero automated review**

These may need manual review during Gate 2 (/dg).

---

## Patterns

1. **CLI test `..` anti-pattern**: When new fields added to `Commands::Serve`, existing tests use `..` and lose default-value coverage.
2. **Comment/code drift**: Comments describing future behavior that was never implemented.
3. **Scope guard incompleteness**: New optional fields not added to guard conditions.
