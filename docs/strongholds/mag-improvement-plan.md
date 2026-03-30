# MAG Improvement Plan

> Stronghold document. Synthesized from 33 memories across 4 sessions + full codebase survey.
> Status: FINAL -- passed 2 rounds of /simplify + /dg
> Date: 2026-03-31
> DG reviews: `improvement-plan-dg-r1.md`, `improvement-plan-dg-r2.md`

---

## Executive Summary

MAG is at v0.1.4 with 90.1% LoCoMo-10 retrieval, 91.2% E2E, 500+ tests, and all issues closed. Retrieval is competitive with AutoMem (90.5%) while being fully local.

The next phase: **make MAG useful without asking the user to do anything.** Storage requires too much manual effort. The goal is "install MAG and benefit naturally."

Two pillars: reduce friction (MCP redesign), then deliver automatic value (preference layer + auto-capture). Competitive moat work is ongoing maintenance, not a separate initiative.

---

## Pillar 1: Reduce Friction (MCP Redesign)

### Problem
16 MCP tools with ~128 schema params = 2-3K extra tokens per API call. MCP-only mode has no hooks, so nothing is automatic.

### Solution

#### 1A. Collapse MCP surface: 16 -> 4 tools

| New Tool | Absorbs | Purpose |
|----------|---------|---------|
| `memory` | store, store_batch, search, retrieve, delete | Core CRUD + search |
| `memory_manage` | update, feedback, relations, lifecycle | Maintenance |
| `memory_session` | session_info, checkpoint, remind, lessons, profile | Session lifecycle |
| `memory_admin` | admin, list | Diagnostics and export |

- Facade over existing internal implementations -- no rewrite
- CLI keeps full 16-command set
- `mag serve --mcp-tools=minimal|full` toggle
- MCP instructions already use 9-prefix priority system
- **Validation gate:** Deploy 4-tool facade to a single MCP client first. Measure task completion rate vs 16-tool mode before committing. Prototype `memory` tool first as a spike.

Each unified tool needs: an `action` discriminator, per-action parameter validation, and clear error messages referencing the sub-operation.

#### 1B. MCP-to-CLI upgrade nudge

- First tool call detects environment, offers CLI+hooks upgrade
- `mag_upgrade` tool installs CLI + plugin
- Progressive: MCP-only -> CLI -> hooks+plugin
- **Limitation:** MCP-only users may not see the nudge reliably (AI must relay system instructions). The realistic upgrade path for most MCP-only users is documentation and README, not in-session AI suggestion. Deprioritize `mag_upgrade` MCP tool; focus on making MCP server instructions clearer.

### Implementation

**Phase 1 (no Rust):** Rewrite MCP instructions to reference collapsed tools. Add upgrade nudge to session_info welcome.

**Phase 2 (Rust):** New `mcp_router.rs` facade with 4 union parameter structs, action discriminators, per-action validation, `--mcp-tools` CLI flag, and tests for all 16 routing paths.

### Effort: 4-5 days | Impact: High (immediate token savings, better onboarding)

---

## Pillar 2: Automatic Value (Preference Layer + Auto-Capture)

### Problem
Users must explicitly ask the AI to store memories. Most never do. Preferences have no hierarchy: global prefs pollute project context. No TTL or dedup on UserPreference. welcome() can reach ~4K tokens at maximum capacity (15 recent memories + 20 preferences + profile + reminders).

### Solution

#### 2A. 4-tier injection hierarchy

| Tier | What | Importance | Behavior |
|------|------|------------|----------|
| 1. Pinned | Critical prefs, always-on | >= 0.9 | Always injected, survives compact |
| 2. Explicit | Normal memories from user | 0.5-0.8 | Injected by relevance |
| 3. Auto-captured | Session summaries, commits, errors | 0.3-0.5 | Surfaces only if highly relevant |
| 4. Raw context | Not stored | N/A | Structured summaries only |

Uses existing `importance` field -- no new columns for MVP. Pinned via `pinned: true` in metadata.

**Token budget enforcement:** welcome() currently has no budget logic. Implementing requires:
- Token counting (chars/4 approximation for MVP)
- Priority ordering: Tier 1 first, then Tier 2 by importance, then Tier 3 by relevance
- 4-tier hierarchy must land first or simultaneously
- Estimated 1-2 days of Rust work

**Token budget hierarchy:** Three independent caps, intentionally different:

| Level | Budget | Purpose |
|-------|--------|---------|
| `welcome()` internal | 3,300 tokens | Trims current ~4K max; priority ordering ensures critical prefs survive |
| `session-start.sh` | 2,000 tokens | Hook-level session injection (deferred to Phase 2; no `--budget-tokens` flag exists today) |
| `compact-refresh.sh` | 800 tokens | Re-inject essentials only after compaction (deferred to Phase 2) |

The hook-level budgets (`session-start.sh`, `compact-refresh.sh`) require either a new `--budget-tokens` flag on `mag welcome` or a `mag hook` subcommand. Both require Rust work and are Phase 2 scope.

#### 2B. Multi-agent scoping

`agent_type` and `entity_id` columns and indexes already exist in schema. Hierarchy: global -> project -> agent-class -> named-agent.

Remaining work: extend `welcome()` to accept `agent_type` + `entity_id` params for scoped briefings.

**Approach:** Add `WelcomeOptions` struct (following the `SearchOptions` pattern) with `session_id`, `project`, `agent_type`, `entity_id`, and `budget_tokens` fields. Add `welcome_scoped(&self, opts: &WelcomeOptions)` to `WelcomeProvider` with a default delegation to existing `welcome()`. No breaking changes.

#### 2C. Auto-capture hooks

**Current state:** The plugin hook scripts (`session-start.sh`, `session-end.sh`, `compact-refresh.sh`) all call `mag hook <subcommand>`, but **`mag hook` does not exist as a CLI command**. The `Commands` enum in `cli.rs` has no `Hook` variant. All three scripts fail silently (`2>/dev/null || true`) and are currently NOPs.

**Phase 1 approach:** Rewrite hook scripts to use existing CLI commands that DO work:

| Hook Script | Current (broken) | Rewritten To |
|-------------|-------------------|--------------|
| `session-start.sh` | `mag hook session-start --project ... --budget-tokens 2000` | `mag welcome --project "$(basename "$PWD")"` |
| `session-end.sh` | `mag hook session-end --project ... --session-id ...` | `mag process "Session summary: ..." --event-type session_end --project ... --session-id ... --importance 0.4` |
| `compact-refresh.sh` | `mag hook compact-refresh --project ... --budget-tokens 800` | `mag welcome --project "$(basename "$PWD")"` (full output; budget trimming deferred to Phase 2) |

The `Stop` hook (`session-end.sh`) needs the AI to pipe a session summary into `mag process`. This requires a prompt-based hook (the Stop hook receives `$CLAUDE_TRANSCRIPT` or similar) that extracts a summary and passes it to `mag process`.

| Hook Event | Captures | Store As | Importance | TTL |
|------------|----------|----------|------------|-----|
| Stop | Session summary | `session_end` | 0.4 | 14 days (SessionEnd default) |
| PostToolUse (jj/git) | Commit descriptions | Decision | 0.5 | 14 days |
| PostToolUse (cargo/npm) | Build/test errors | ErrorPattern | 0.5 | None (permanent) |

**Session summary TTL:** Stored as `session_end` event type, which has `TTL_LONG_TERM` (14 days). This is intentional: session summaries become stale, and permanent storage causes unbounded growth. No `--ttl none` override.

**PostToolUse hooks:** New territory for the plugin -- `hooks.json` currently has only `SessionStart`, `UserPromptSubmit`, `PostCompact`, and `Stop`. PostToolUse fires on every tool use, requiring fast pattern matching (<50ms), output parsing, and new `hooks.json` entries. This is 1.5-2.5 days of work, not trivial shell scripts.

**Telemetry:** Hooks log auto-capture events to `~/.mag/auto-capture.log` (timestamp, event type, memory ID). Logging must happen in the shell script itself (not from `mag` output) to ensure failed invocations are also recorded.

Reranker naturally demotes auto-captured (low importance) vs explicit stores.

#### 2D. Compact survival

PostCompact hook re-injects pinned preferences. compact-refresh.sh already exists (currently a NOP); rewrite to call `mag welcome`. Critical prefs also dual-written to CLAUDE.md (immune to compaction).

### Implementation

**Phase 1 (hooks/skills, no Rust) -- 2-3 days:**
- Rewrite `session-start.sh` to call `mag welcome --project ...` (0.25 day)
- Rewrite `session-end.sh` Stop hook to generate summary and call `mag process --event-type session_end` (1 day)
- PostToolUse commit-capture hook for jj/git (1-1.5 days)
- PostToolUse error-capture hook for cargo/npm (0.5-1 day)
- Rewrite `compact-refresh.sh` to call `mag welcome --project ...` (0.25 day)
- Auto-capture telemetry logging in hook scripts (0.25 day)
- Skill rewrites for scoping conventions (0.5 day)

**Phase 2 (Rust) -- 3-5 days:**
- `WelcomeOptions` struct + `welcome_scoped()` trait method (1 day)
- Token budget cap with chars/4 counting and 4-tier priority ordering (1-2 days)
- `--budget-tokens` flag on `mag welcome` for hook-level caps (0.5 day)
- UserPreference dedup threshold (Jaccard >= 0.75, consider cosine as primary) (0.5 day)
- `last_confirmed_at` column for staleness detection (0.5 day)

### Effort: Phase 1 = 2-3 days, Phase 2 = 3-5 days | Impact: Very High

---

## Ongoing: Competitive Maintenance

These are continuous improvements, not a separate pillar. They happen alongside Pillars 1-2.

**Scoring:** Graph enrichment factor tuning (currently 0.1, test 0.15-0.2 with benchmark gate).

**Benchmarks:** Add Hit@K metric (cheap -- evidence_recall already tracks dia_ids). Evaluate memorybench compatibility (2-hour feasibility check) before adopting. Always report both retrieval (90.1%) and E2E (91.2%).

**Cross-tool:** Tier 2 tools (Codex CLI, OpenCode) need skill installation via `install_skills()` in setup.rs. Rules file generation for Cursor/Windsurf is low priority. Auto-capture hooks only benefit Claude Code users; other tools remain MCP-only.

---

## Implementation Sequence

### Wave 1: Hooks & Skills (no Rust) -- 2-3 days
Zero risk to existing functionality.
1. Rewrite `session-start.sh` and `compact-refresh.sh` to call `mag welcome` (replacing broken `mag hook` calls)
2. Rewrite `session-end.sh` Stop hook to summarize and call `mag process --event-type session_end`
3. PostToolUse commit-capture hook (jj/git) -- new hook event, matcher, parser
4. PostToolUse error-capture hook (cargo/npm) -- new hook event, matcher, parser
5. Auto-capture telemetry logging (in-script, before `mag` calls)
6. Skill rewrites for scoping conventions

### Wave 2: MCP Facade + Preference Engine (Rust) -- 5-7 days
Requires benchmark gates.
1. MCP router facade: prototype `memory` tool first as spike, then build remaining 3 unified tools
2. `--mcp-tools` flag
3. `WelcomeOptions` struct + `welcome_scoped()` trait method
4. Token budget cap with chars/4 counting and 4-tier priority ordering
5. `--budget-tokens` flag on `mag welcome`
6. UserPreference dedup threshold
7. Hit@K benchmark metric

### Deferred (no current need)
- Learned scorer (no usage data -- instrument recall_feedback first)
- Confidence field / observation counting
- Prospective indexing (cross-encoder covers similar ground at read time)
- RRF fusion redesign (parameter tuning hurts; architecture is fine)
- Embedding model switch (tested 16+, no improvement justifies it)
- Visibility field (private/project/global) -- overkill for solo dev
- `preference_level` column -- importance field + metadata `pinned: true` covers this
- Concurrency stress testing for multi-agent SQLite (see Risk Registry)

---

## Risk Registry

| Risk | Mitigation |
|------|------------|
| MCP facade breaks integrations or causes routing errors | `--mcp-tools=full` default; prototype `memory` tool as spike; A/B test vs 16-tool mode before committing |
| Auto-capture floods low-quality memories | Importance 0.4-0.5 + reranker demotion; telemetry to monitor volume |
| Schema migration breaks DBs | Additive only (AGENTS.md convention); benchmark suite on new schema before shipping |
| Token budget cap truncates context | 3,300 trims current ~4K max; 4-tier priority ensures critical prefs always included |
| PostToolUse hooks too slow | Must finish in <50ms; fast-path rejection for non-matching tools |
| Multi-agent SQLite contention | `retry_on_lock()` (5 attempts, 10-80ms base + 0-50% jitter, max ~120ms) handles casual concurrency. Monitor p99; revisit if retries exceed 2% of writes |

---

## Success Metrics

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| MCP token overhead | ~2-3K tokens | ~500 tokens | Schema tokens in 4-tool mode |
| Manual store calls needed | ~5-10/session | ~1-2/session | Auto-capture telemetry log analysis |
| LoCoMo-10 retrieval | 90.1% | >= 90.0% (no regression) | `./scripts/bench.sh --samples 10` |
| Auto-captured memories per session | 0 | >= 3 (summary + commits/errors) | Auto-capture telemetry log counts |
| welcome() includes auto-context | No | Yes (auto-captured memories surface in briefing) | Manual verification: `mag welcome` output contains auto-captured entries |

---

## Assumptions

1. MAG's retrieval handles noisy auto-captured memories -- validated at 90.1%
2. Single SQLite DB is fine for multi-agent -- isolation via filtering, not sharding
3. Claude Code hook system is stable (SessionStart, PostToolUse, PostCompact, Stop)
4. Solo developer -- estimates assume single-person execution with agent orchestration
5. Auto-capture hooks only benefit Claude Code users; other tools remain MCP-only

---

## Change Log

Full DG Round 1 finding-by-finding resolution: see `docs/strongholds/improvement-plan-revision-log.md`.
Full Simplification Round 1 changes: see same file.

### Simplification Round 2 (2026-03-31)

- **Extracted revision and simplification logs** to separate file. Audit trail was ~80 lines (25% of document) -- useful for traceability but not for decision-making.
- **Removed DG finding tags** (DG-H1, DG-M3, etc.) from plan body. The DG review document is the canonical reference; inline tags added noise to every section.
- **Consolidated two MCP routing risks** into one row ("breaks integrations" and "routing errors" are the same risk with the same mitigation).
- **Trimmed implementation justifications.** Removed explanations of what does/doesn't exist in domain.rs (ErrorPattern abstention methods, etc.) -- these justify decisions already made. The decision stands without the derivation.
- **Condensed token budget section** into a table format. Replaced two prose paragraphs explaining the hierarchy with a 3-row table.
- **Trimmed WelcomeProvider section.** Removed Rust trait signature block and alternative approach -- the chosen approach is stated, that's sufficient.
- **Removed DG adjustment annotations** from effort estimates. The estimates are now the real estimates; their history is in the revision log.
- **Cut rmcp proc macro details** from Phase 2 implementation. Implementation-level concerns belong in a spec, not the strategic plan.
- **Net reduction:** ~90 lines removed, no information lost (all moved to revision log or already present in the DG review document).

### R2 Fixes (2026-03-31)

Incorporated findings from DG Round 2 (`improvement-plan-dg-r2.md`):

- **H1 (HIGH): `mag hook` doesn't exist.** All hook scripts called `mag hook <subcommand>` which is not a CLI command -- scripts were silent NOPs. Rewrote Phase 1 to use existing CLI commands: `session-start.sh` and `compact-refresh.sh` call `mag welcome`, `session-end.sh` calls `mag process --event-type session_end`. Added migration table showing old -> new commands. Deferred `--budget-tokens` flag to Phase 2 Rust work.
- **M1 (MEDIUM): TTL contradiction.** Section 2C table said "TTL_LONG_TERM (14 days)" but Phase 1 implementation said `--ttl none` (permanent). Resolved: 14-day TTL is correct (session summaries become stale). Removed `--ttl none` override.
- **M2 (MEDIUM): welcome() 9K claim inflated.** Actual maximum is ~4K tokens (15 * ~350 chars + 20 * ~450 chars + ~1200 chars = ~15,450 chars / 4). Fixed claim and clarified that 3,300-token budget means trimming the current max, not preventing explosion.
- **M3 (MEDIUM): retry_on_lock backoff range.** Changed "10-160ms + jitter" to "10-80ms base + 0-50% jitter, max ~120ms" matching actual implementation in conn_pool.rs.
- **L1 (LOW): Unmeasurable success metric.** Replaced "Time to first useful recall" with two concrete metrics: "Auto-captured memories per session" (telemetry) and "welcome() includes auto-context" (verification).
- **L2 (LOW): Upgrade nudge limitation.** Added note that MCP-only nudge is best-effort; realistic upgrade path is documentation, not in-session AI suggestion. Deprioritized `mag_upgrade` MCP tool.
