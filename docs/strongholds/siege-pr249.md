# Siege Stronghold: PR #249

## PR Info
- **Branch:** fix/plugin-hooks-and-welcome-recall
- **Title:** fix(plugin,welcome): fix hooks not firing and add semantic recall to welcome

## Loop Control
- **Status:** active
- **Round:** 5 / 10 max
- **Action:** CONTINUE
- **Dispatch:** idle
- **CI_Fail_Streak:** 0

---

### Siege Round 3 -- 2026-04-08T06:34:00Z
**Status:** complete
**Dispatch:** idle
Fixes applied: 3
Commits: 71ac7b2f1c8ea4a70d713af814a0d917fc31cb8c
Deferred: 0
Blocked: none

**Fix 1 (dedup set):** Made `seen_ids` mutable at both `welcome()` (L989) and `welcome_scoped()` (L1247) call sites. Inserted accepted record IDs into the set during semantic insert loops so intra-batch duplicates are properly filtered.

**Fix 2 (error telemetry):** Replaced silent `if let Ok` with `match` at both call sites. Err branch now emits `tracing::debug!` with project context, query length, candidate count, and error kind. No raw content logged.

**Fix 3 (test assertion):** Strengthened `test_welcome_scoped_semantic_search`: stores 55 memories (exceeding tier LIMIT 50) so semantic phase produces unique results. Added `"source": "tiered"` to SQL-tier results. Test asserts every item has source field and at least one is `"semantic"`.

All 3 threads replied and resolved. CodeRabbit CHANGES_REQUESTED review dismissed.

### Tick 2026-04-08T07:00:00Z -- Round 4
**State:** NEEDS_FIXES
**CI:** pass (all 6 checks green)
**Unresolved threads:** 4 (0 outdated)
**Action taken:** Dispatching Siege Commander Round 4

## Round 4 Threads

### Thread 1 — SearchOptions construction (Trivial)
- **File:** `src/memory_core/storage/sqlite/admin.rs:987`
- **ThreadID:** `PRRT_kwDORlN5NM55etdI`
- **CommentDBID:** `3049626794`
- **Issue:** Use `SearchOptions::default()` construction pattern (repo convention).
- **Status:** pending

### Thread 2 — Semantic query uses project label (Major)
- **File:** `src/memory_core/storage/sqlite/admin.rs:1000`
- **ThreadID:** `PRRT_kwDORlN5NM55etdQ`
- **CommentDBID:** `3049626803`
- **Issue:** `advanced_search` is called with the project string as the query, which makes recall depend on stored content containing that label. Should use recall-oriented terms or aggregated memory text. Also applies to ~L1246-1258.
- **Status:** pending

### Thread 3 — Source tagging inconsistent (Major)
- **File:** `src/memory_core/storage/sqlite/admin.rs:1024`
- **ThreadID:** `PRRT_kwDORlN5NM55etdR`
- **CommentDBID:** `3049626805`
- **Issue:** `welcome()` adds semantic items with `source = "semantic"` but SQL-tier rows have no `source` field. `welcome_scoped()` tags tiered as `"tiered"`. Response shape varies based on whether budget_tokens is supplied.
- **Status:** pending

### Thread 4 — Edge-case test assertions weak (Minor)
- **File:** `src/memory_core/storage/sqlite/tests.rs:4223`
- **ThreadID:** `PRRT_kwDORlN5NM55etdV`
- **CommentDBID:** `3049626809`
- **Issue:** Budget test should assert empty results (200-token overhead > 100 budget). Dedup test should assert non-empty before checking dedup.
- **Status:** pending

### Review to Dismiss
- **ReviewID:** `PRR_kwDORlN5NM7yyMh3` (DBID: `4073244791`) — coderabbitai CHANGES_REQUESTED

---

### Siege Round 4 -- 2026-04-08T07:11:00Z
**Status:** complete
**Dispatch:** idle
Fixes applied: 4
Commits: 69c0365e4829140e8bea5182f6a5e13c194b2e5f
Deferred: 0
Blocked: none

**Fix 1 (SearchOptions construction):** Kept struct init with `..SearchOptions::default()` at both call sites -- clippy's `field_reassign_with_default` lint rejects the `default() + field mutation` pattern, so the struct init form is the idiomatic Rust approach. Replied explaining clippy constraint.

**Fix 2 (semantic query):** Replaced raw project label as semantic query with content-derived snippets from tiered results. Takes first 5 tiered result contents (~100 chars each), concatenates into "recent context: <snippets>". Falls back to "recent important memories" when tiered results are empty. Applied at both welcome() and welcome_scoped() call sites.

**Fix 3 (source tagging):** Added `"source": "tiered"` to both SQL-tier result constructions in welcome() -- the recent memories query and the user preferences query. Response shape now consistent across welcome() and welcome_scoped().

**Fix 4 (test assertions):** Budget test now asserts results are completely empty when budget < overhead. Dedup test asserts non-empty before checking IDs.

All 4 threads replied and resolved. CodeRabbit CHANGES_REQUESTED review dismissed. Quality gates pass (fmt, clippy, 462 unit tests). CLI smoke test fails due to HuggingFace 500 -- unrelated external service issue.

---

### Siege Round 5 -- 2026-04-08T07:35:00Z
**Status:** complete
**Dispatch:** idle
Fixes applied: 3
Commits: dad505f55037cb57dad772ccaa4916289ebdf426
Deferred: 0
Blocked: none

**Fix 1 (OVERHEAD_TOKENS constant):** Moved `const OVERHEAD_TOKENS: usize = 200` from inside `spawn_blocking` closure to `welcome_scoped()` function scope. Removed duplicate `let overhead_tokens: usize = 200` local variable in semantic search phase; now uses the shared constant.

**Fix 2 (sanitize log):** Replaced `query = semantic_query.as_str()` with `project = opts.project.as_deref().unwrap_or("")` in `tracing::debug!` error path of `welcome_scoped()`. Now matches `welcome()` pattern: logs project + query_len, no user content.

**Fix 3 (strict dedup test):** Changed `filter_map(|m| m["id"].as_str())` to `map(|m| m["id"].as_str().expect(...))` so missing `id` fields cause immediate test failure instead of being silently dropped.

All 3 threads replied and resolved. CodeRabbit CHANGES_REQUESTED review dismissed. Quality gates pass (fmt, clippy, all tests green).
