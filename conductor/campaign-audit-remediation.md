# Campaign: Codebase Audit Remediation

**Issues:** #115-#127
**Started:** 2026-03-27
**Status:** 9 of 13 shipped, 1 forging, 3 deferred

## Wave Structure

### Wave 1 — Critical Security (parallel, in worktrees)
| Issue | Title | Status | Approach |
|-------|-------|--------|----------|
| #115 | Timing side-channel in constant_time_eq | IMPLEMENTING | SHA-256 hash comparison; sha2 already a dep |
| #116 | Input validation limits (DoS) | IMPLEMENTING | 23 params need bounds; MAX_RESULT_LIMIT=1000 |
| #127 | Unsafe transmute in sqlite-vec | SCOUTING | Check sqlite-vec API for typed fn pointer |

### Wave 2 — Bug Fixes (parallel)
| Issue | Title | Status | Approach |
|-------|-------|--------|----------|
| #117 | cosine_similarity naming + source_type | IMPLEMENTING | Rename to dot_product (all callers normalize); add source_type to MemoryInput |

### Wave 3 — Architecture Refactors (sequential: #120 → #118+#122 → #119 → #123)
| Issue | Title | Status | Approach |
|-------|-------|--------|----------|
| #120 | Remove dead abstractions | SPEC READY | Gate auth/daemon/idle_timer behind feature; remove PlaceholderPipeline extras; remove InitMode::Advanced |
| #118 | Split god modules | SPEC READY | mod.rs→types.rs+traits.rs+pipeline.rs; helpers.rs→6 focused files |
| #122 | Deduplicate code patterns | SPEC READY | Activate resolve_priority helper (5 copies→1); extract collect_candidates; flatten get_synonyms |
| #119 | Separate scoring from storage | SPEC READY | Extract scoring module; HIGH RISK for benchmark regression |
| #123 | Schema version tracking | SPEC READY | Add schema_migrations table; detect existing version from columns; 14 ALTERs→versioned |

### Wave 4 — Performance (#122 dedup MUST precede)
| Issue | Title | Status | Approach |
|-------|-------|--------|----------|
| #121 | Hot-path inefficiencies | SPEC READY | Parallelize sub-queries; prepare once outside loop; cache token_set; batch graph queries |

### Wave 5 — Testing (can overlap with Wave 3-4)
| Issue | Title | Status | Approach |
|-------|-------|--------|----------|
| #126 | Flaky test fixes | SPEC READY | tokio::time::pause(); serial_test for env vars; remove sleeps |
| #124 | MCP protocol coverage | SPEC READY | Cover remaining 9/16 tools; add edge cases, unicode |
| #125 | Property/fuzz/stress tests | SPEC READY | Add proptest for scoring; fuzz targets for import/search/stem |

## Key Decisions

1. **#115**: Use SHA-256 approach (no new dep) over subtle crate
2. **#117**: Rename to `dot_product` not `dot_product_normalized` (simpler)
3. **#119**: HIGH RISK — must run full LoCoMo benchmark before/after
4. **#120**: Gate dead modules behind `daemon-http` feature flag (don't delete — HTTP server is planned)
5. **#122**: Priority resolution — activate existing dead helper in helpers.rs:1042
6. **#123**: Use sequential integer versioning (v0-v4 for existing schema)

## Implementation Protocol

Each work unit follows:
1. `jj describe -m "type(scope): description (#issue)"` — frequently during work
2. Quality gates: `prek run` (fmt + clippy + test)
3. `/simplify` review
4. `jj bookmark set <branch> -r @- && jj git push --bookmark <branch> --allow-new`
5. `gh pr create` with issue reference
6. Merge immediately on green

## Findings Log

- scout-116: 23 unbounded parameters in MCP server, no validation infrastructure exists
- scout-118-120: 27 traits in mod.rs but only SqliteStorage implements them; PlaceholderPipeline is dead weight
- scout-121-122: Priority resolution duplicated 5x; dead resolve_priority helper at helpers.rs:1042; conn.prepare inside loop at advanced.rs:890
- scout-123: 14 ALTER TABLE + 20 CREATE INDEX run on EVERY DB open; no version tracking
- scout-124-126: 7/16 MCP tools tested; no proptest/fuzz deps; 2 sleep-based flaky tests
