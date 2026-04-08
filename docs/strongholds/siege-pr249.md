# Siege Stronghold: PR #249

## PR Info
- **Branch:** fix/plugin-hooks-and-welcome-recall
- **Title:** fix(plugin,welcome): fix hooks not firing and add semantic recall to welcome

## Loop Control
- **Status:** active
- **Round:** 4 / 10 max
- **Action:** CONTINUE
- **Dispatch:** active-round-4 (claimed: 2026-04-08T07:10:00Z)
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
