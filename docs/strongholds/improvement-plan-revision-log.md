# MAG Improvement Plan -- Revision Log

> Audit trail for changes to `docs/strongholds/mag-improvement-plan.md`.
> DG review: `docs/strongholds/improvement-plan-dg-r1.md`

---

## DG Round 1 Resolutions (2026-03-31)

Incorporated findings from the DG adversarial review. Each finding and its resolution:

### CRITICAL

| Finding | Resolution |
|---------|------------|
| **C1. Supersession "fix" already implemented** | Already removed in prior /simplify pass. Confirmed: `SUPERSESSION_COSINE_THRESHOLD = 0.70` as primary gate, `SUPERSESSION_JACCARD_THRESHOLD = 0.30` as secondary, in `src/memory_core/storage/sqlite/mod.rs` lines 55-60 and `crud.rs` lines 196-219. No action needed. |
| **C2. SessionSummary has TTL_EPHEMERAL (1 hour)** | Confirmed: `domain.rs` line 114 maps `SessionSummary => Some(TTL_EPHEMERAL)` = 1 hour. Auto-captured summaries would self-destruct before next session. **Fix: store as `SessionEnd` event type** which has `TTL_LONG_TERM` (14 days). Semantically correct and requires no Rust changes -- just use `--event-type session_end` in the hook script. |
| **C3. PostToolUse hooks don't exist in plugin** | Confirmed: `hooks.json` has only `SessionStart`, `UserPromptSubmit`, `PostCompact`, `Stop`. No PostToolUse entries. **Fix: reclassified from "trivial shell scripts" to "new hook infrastructure."** Broken out with proper effort estimate (1.5-2.5 days for both commit and error capture). Updated Phase 1 effort from 1 day to 2-3 days. |

### HIGH

| Finding | Resolution |
|---------|------------|
| **H1. MCP 4-tool collapse risks accuracy** | Added A/B validation gate to Section 1A. Added "prototype `memory` tool first" spike to Wave 2 sequence. Added routing error risk to Risk Registry. |
| **H2. welcome() has no token budget enforcement** | Confirmed: `admin.rs` lines 888-989 show no token counting anywhere. **Fix: sized as 1-2 days of standalone Rust work.** Specified chars/4 counting strategy and dependency on 4-tier hierarchy. Added token budget hierarchy section. |
| **H3. WelcomeProvider trait signature change** | Confirmed: trait has only `session_id` and `project` params. **Fix: use `WelcomeOptions` struct pattern** (matching existing `SearchOptions` convention). Add `welcome_scoped()` as a NEW trait method with default delegation to existing `welcome()`. No breaking change. |
| **H4. Multi-agent SQLite concurrency untested** | Acknowledged as real but not a blocker. `retry_on_lock()` with bounded backoff handles casual concurrency. Added to Risk Registry with monitoring threshold (2% retry rate). Moved stress testing to Deferred list. |
| **H5. No rollback plan for schema migrations** | `preference_level` column was already removed in /simplify pass. `last_confirmed_at` is the only remaining new column -- nullable timestamp with no query impact if unused. Added benchmark validation to Risk Registry mitigation. |

### MEDIUM

| Finding | Resolution |
|---------|------------|
| **M1. Cross-tool "Done" status misleading** | Added note to Ongoing section: auto-capture only benefits Claude Code. Added Assumption #5. |
| **M2. ErrorPattern at importance 0.3 is invisible** | **Fix: raised to 0.5.** DG review incorrectly cited `abstention_min() => Some(0.70)` on ErrorPattern -- no such per-type method exists in domain.rs. Actual abstention gate is global `abstention_min_text: 0.15`. Ranking concern is valid regardless. |
| **M3. Token budget hierarchy mismatch** | Added explicit hierarchy to Section 2A. welcome() at 3,300, session-start at 2,000, compact-refresh at 800. Intentionally different. |
| **M4. MCP facade effort underestimated** | **Fix: total Pillar 1 effort revised from 2-3 days to 4-5 days.** Phase 2 alone is 3-4 days due to rmcp proc macro complexity, union type schemas, and routing tests. |
| **M5. memorybench has no feasibility assessment** | Added 2-hour feasibility check before adopting. |
| **M6. No auto-capture telemetry** | Added telemetry work item to Wave 1: hooks log to `~/.mag/auto-capture.log`. |

### LOW

| Finding | Resolution |
|---------|------------|
| **L1. UserPreference dedup Jaccard threshold** | Noted in Phase 2: "consider cosine as primary signal" for preference dedup. |
| **L2. Windows/Linux support** | Acknowledged via Assumption #5. POSIX-only hooks acceptable for solo dev phase. |
| **L3. Codex CLI skill dirs unverified** | Already removed from Assumptions in /simplify pass. Verify at implementation time. |

### Effort Summary After DG Revision

- Wave 1 (hooks, no Rust): 1-2 days -> **2-3 days** (+50%)
- Wave 2 (Rust): 3-4 days -> **5-7 days** (+60%)
- **Total: 4-6 days -> 7-10 days** (honest estimate for solo dev with agent orchestration)

---

## Simplification Round 1 (2026-03-31)

### Structural changes
- **Collapsed 3 pillars to 2 + ongoing maintenance.** Pillar 3 (Competitive Moat) was not a strategic initiative -- it was routine scoring/benchmark/cross-tool work. Demoted to "Ongoing: Competitive Maintenance" section.
- **Collapsed 4 waves to 2.** Original Waves 2-3 had an artificial split between "MCP Facade + Scoring" and "Schema + Welcome Extensions." Two waves: hooks-only (safe), then Rust (requires gates).
- **Renamed "Layers" to "Phases"** within each pillar for consistency with waves.

### Removed redundancy
- **Supersession cosine fix removed.** Codebase already uses `SUPERSESSION_COSINE_THRESHOLD` as primary signal in crud.rs.
- **`agent_type` index removed from Wave 3.** Schema already has `agent_type` column, `entity_id` column, and `idx_memories_entity_id` index.
- **Distribution funnel (1B) simplified.** Removed "Remind later" state persistence -- unnecessary complexity for a one-time nudge.

### Cut unnecessary complexity
- **Removed `preference_level` column.** The importance field already serves this purpose.
- **Removed `pinned` boolean flag.** Metadata JSON `pinned: true` achieves the same result without a schema migration.
- **Removed Layer 3 entirely from Pillar 2.** Confidence field, visibility field, and recall_feedback instrumentation were all deferred -- moved to single Deferred list.

### Tightened language
- Removed hedging ("Key insight from benchmark sessions").
- Removed "Leaderboard reality" paragraph -- competitive positioning belongs in marketing.
- Removed Assumption #4 (Codex CLI skill dirs) -- verify at implementation time.
- Removed cross-tool tier table -- only Tier 2 needs work.
- Cut "Supported AI tools" success metric -- vanity metric.

### Risk registry
- Removed "Supersession cosine threshold too aggressive" -- already implemented and validated.
- Removed "Cross-tool skill dirs change locations" -- handle at implementation time.
