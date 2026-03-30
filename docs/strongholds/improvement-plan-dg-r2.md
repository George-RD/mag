# Adversarial Review: MAG Improvement Plan (Round 2)

> Reviewer: Devil's Advocate (DG Round 2)
> Date: 2026-03-31
> Document under review: `docs/strongholds/mag-improvement-plan.md` (post-R1 revision)
> Sources: Full codebase re-survey, CLI binary verification, domain.rs TTL mappings, hooks.json, admin.rs welcome(), conn_pool.rs retry logic, scoring.rs parameters
> Focus: Residual incorrectness, logical consistency, edge cases, metric validity, token budget math, upgrade funnel reality

---

## Overall Verdict: CONDITIONAL PASS

Round 1 caught the structural and factual problems. The revision addressed them well. Round 2 found no new CRITICAL issues but identified **1 HIGH** issue (the plan's foundation assumes `mag hook` CLI subcommands exist -- they do not), **3 MEDIUM** issues (contradictory TTL specification, inflated welcome() token claim, retry backoff range inherited from AGENTS.md), and **2 LOW** issues (success metric measurability, upgrade nudge delivery gap).

The plan is implementable after fixing the HIGH finding. The MEDIUM findings are imprecisions that should be corrected to avoid confusion during implementation.

---

## Findings

### HIGH

#### H1. `mag hook` CLI subcommand does not exist -- plan's entire hook infrastructure rests on a nonexistent command (Confidence: 95)

**Evidence:** The plugin hook scripts all invoke `mag hook <subcommand>`:
- `plugin/scripts/session-start.sh` line 4: `mag hook session-start --project ... --budget-tokens 2000`
- `plugin/scripts/session-end.sh` line 5: `mag hook session-end --project ... --session-id ...`
- `plugin/scripts/compact-refresh.sh` line 3: `mag hook compact-refresh --project ... --budget-tokens 800`

Running `cargo run -- hook --help` returns `error: unrecognized subcommand 'hook'`. The `src/cli.rs` `Commands` enum has no `Hook` variant. No Rust source file contains the string `"hook"` as a command. The `daemon-http` feature (which once provided hook infrastructure via `hook_handlers.rs` and `hook_client.rs`) is not a default feature, and those source files have been removed from the tree.

All three hook scripts use `2>/dev/null || true`, which means they fail silently. **The existing hook scripts are currently NOPs** -- they produce no output and store nothing.

**Impact on plan:** The plan's Phase 1 says "Enhance existing session-end.sh to store session summary" (line 136) and discusses `--budget-tokens` flags that map to `mag hook` subcommands (line 90). This assumes a working CLI backend that does not exist. Phase 1 is not "hooks/skills, no Rust" -- it requires either:
- (a) Implementing the `mag hook` subcommand group in Rust (adding a `Hook` variant to `Commands` with sub-subcommands for `session-start`, `session-end`, `compact-refresh`, `store`, `search`), OR
- (b) Rewriting the hook scripts to use existing CLI commands (`mag welcome`, `mag process`, `mag advanced-search`) directly

Option (b) is feasible without Rust changes but changes the semantics: `session-start.sh` would call `mag welcome --project ...` (which exists), `session-end.sh` would call `mag process "session summary" --event-type session_end ...` (which exists). But `--budget-tokens` has no equivalent in any existing command.

**Recommended fix:** Add a section acknowledging that `mag hook` is unimplemented scaffolding. Either:
1. Scope Phase 1 to use existing CLI commands (`mag welcome`, `mag process`) as the backend, accepting no token budget enforcement until Phase 2 Rust work
2. Move `mag hook` subcommand implementation into Phase 1 and reclassify Phase 1 as "hooks + light Rust" with +1-2 days effort
3. Option (b) + defer budget-tokens to Phase 2

This also affects the token budget hierarchy discussion (Section 2A, lines 86-90) which describes `--budget-tokens` as if it's a working flag.

---

### MEDIUM

#### M1. Contradictory TTL specification for auto-captured session summaries (Confidence: 85)

**Evidence:** Two places in the plan specify the TTL for auto-captured session summaries, and they disagree:

- **Section 2C table** (line 110): `SessionEnd` with `TTL_LONG_TERM (14 days)`
- **Phase 1 implementation** (line 136): "store session summary as `session_end` event type with `--ttl none` override"

`--ttl none` means permanent (no expiry). `TTL_LONG_TERM` means 14 days. These are contradictory. The table says 14-day TTL; the implementation step says override to permanent.

If the intent is to use `SessionEnd`'s default TTL (14 days, confirmed at `domain.rs` line 124: `EventType::SessionEnd => Some(TTL_LONG_TERM)`), then remove the `--ttl none` override from the implementation step. If the intent is permanent storage, update the table to say "None (permanent)" instead of "TTL_LONG_TERM (14 days)".

14 days is likely the better choice: session summaries become stale, and permanent storage would cause unbounded growth.

**Recommended fix:** Remove `--ttl none` from line 136. Let `SessionEnd`'s default 14-day TTL apply naturally.

---

#### M2. "welcome() can exceed 9K tokens" claim is inflated by approximately 2.5x (Confidence: 82)

**Evidence:** The plan states in Section 2A (line 64): "welcome() can exceed 9K tokens." Analysis of the actual `welcome()` implementation (`admin.rs` lines 888-989):

- Recent memories: 15 entries, content truncated to 200 chars each, ~350 chars per entry with JSON overhead = ~5,250 chars
- User preferences: 20 entries, content truncated to 300 chars each, ~450 chars per entry with JSON overhead = ~9,000 chars
- Profile, reminders, greeting: ~1,200 chars
- **Total maximum: ~15,450 chars = ~3,862 tokens** (using the plan's own chars/4 approximation)

Even with generous JSON overhead accounting, the maximum output is approximately 3,800-4,000 tokens. 9K tokens would require ~36,000 characters, which is 2.3x the theoretical maximum output of the function.

The plan's proposed 3,300-token budget cap (line 77) is actually close to the current maximum, meaning the budget enforcement may rarely trigger. This doesn't invalidate the feature (it provides a safety net), but the urgency framing ("can exceed 9K") is misleading.

**Recommended fix:** Replace "can exceed 9K tokens" with "can reach ~4K tokens at maximum capacity (15 recent + 20 preferences + profile + reminders)." Note that the 3,300-token budget would require truncation of the current maximum, making the priority ordering logic meaningful.

---

#### M3. retry_on_lock backoff range "10-160ms" is incorrect (Confidence: 80)

**Evidence:** The plan (line 212) and AGENTS.md both state `retry_on_lock()` uses "5 attempts, 10-160ms + jitter". The actual implementation in `conn_pool.rs` lines 36-63:

- `RETRY_MAX_ATTEMPTS = 5`
- `RETRY_BASE_DELAY_MS = 10`
- Backoff formula: `base_ms = 10 * 2^(attempt-1)` where attempt goes 1..4
- Delays: 10ms, 20ms, 40ms, 80ms base
- Jitter: 0-50% of base_ms
- **Maximum delay: 80ms + 40ms jitter = 120ms** (not 160ms)

This is inherited from AGENTS.md and is a minor imprecision, but the plan should not amplify incorrect source documentation.

**Recommended fix:** Change "10-160ms + jitter" to "10-80ms base + 0-50% jitter (max ~120ms)" in the plan. Optionally fix AGENTS.md too.

---

### LOW

#### L1. Success metric "Time to first useful recall" is not measurable (Confidence: 75)

**Evidence:** The success metrics table (line 224) includes:

| Metric | Current | Target | Measurement |
|--------|---------|--------|-------------|
| Time to first useful recall | ~5 min (manual) | ~0 (automatic) | User experience |

"User experience" is not a measurement method. This metric has no instrumentation path. Unlike "Manual store calls needed" (which can use auto-capture telemetry logs, per DG-M6), there is no mechanism to measure when a "useful recall" occurs or how long it takes.

The other three metrics are measurable: schema token count, telemetry log counts, benchmark scores. This one is a qualitative goal masquerading as a metric.

**Recommended fix:** Either replace with a measurable proxy (e.g., "Percentage of sessions with auto-injected context at start: Current 0% -> Target 100%") or move to a "Qualitative Goals" section separate from measured metrics.

---

#### L2. MCP-to-CLI upgrade nudge has no delivery mechanism for MCP-only users (Confidence: 72)

**Evidence:** Section 1B (line 46) says "First tool call detects environment, offers CLI+hooks upgrade" and the MCP server instructions (`mcp_server.rs` lines 83-87) already include upgrade text:

```
MAG works best with the CLI + hooks plugin, which provides automatic memory
at session start/end, after compaction, and on every prompt...
```

However, this text is in the MCP `initialize` handshake instructions, which are system-level context the AI reads but the **user** never sees directly. The nudge depends on the AI model choosing to relay this message to the user, which is unreliable:
- The instruction says "mention this once per session" but provides no enforcement
- Many AI clients suppress or truncate system instructions
- The proposed `mag_upgrade` tool (line 47) doesn't exist yet, and MCP-only users can't install CLI tools through an MCP tool call without shell access

The plan's `mag_upgrade` tool concept is reasonable but underspecified: how does an MCP tool install a CLI binary? It would need to invoke shell commands, which most MCP clients don't support.

**Recommended fix:** Acknowledge that the upgrade nudge is best-effort for MCP-only users. The realistic upgrade path is: user reads docs or README, not an in-session AI suggestion. Deprioritize `mag_upgrade` MCP tool; focus on making the existing MCP instructions clearer instead.

---

## Items Verified as Correct

The following plan claims were verified against source code and found accurate:

1. **SessionSummary TTL**: Confirmed `EventType::SessionSummary => Some(TTL_EPHEMERAL)` at `domain.rs` line 114. TTL_EPHEMERAL = 3600 (1 hour). The DG-C2 fix (store as SessionEnd instead) is correct.

2. **SessionEnd TTL**: Confirmed `EventType::SessionEnd => Some(TTL_LONG_TERM)` at `domain.rs` line 124. TTL_LONG_TERM = 1,209,600 (14 days). Correct.

3. **ErrorPattern has no TTL**: Confirmed `EventType::ErrorPattern => None` at `domain.rs` line 116. The table's "None (permanent)" for error captures is correct.

4. **ErrorPattern type_weight**: Confirmed `EventType::ErrorPattern => 2.0` at `domain.rs` line 147. Plan correctly notes this helps compensate for lower importance.

5. **Supersession thresholds**: Confirmed `SUPERSESSION_COSINE_THRESHOLD = 0.70` at `mod.rs` line 55 and `SUPERSESSION_JACCARD_THRESHOLD = 0.30` at `mod.rs` line 60. Revision log accurately describes the already-implemented state.

6. **WelcomeProvider trait signature**: Confirmed at `traits.rs` lines 243-249: `async fn welcome(&self, session_id: Option<&str>, project: Option<&str>)`. Plan's WelcomeOptions struct approach is sound.

7. **welcome() has no token budget logic**: Confirmed at `admin.rs` lines 888-989. No character counting, no token estimation, no truncation beyond per-entry limits (200/300 chars). The function fetches all qualifying rows up to hard LIMIT caps.

8. **hooks.json has only 4 events**: Confirmed: `SessionStart`, `UserPromptSubmit`, `PostCompact`, `Stop`. No PostToolUse entries. Plan correctly identifies this as new infrastructure.

9. **Abstention gate**: Confirmed `abstention_min_text: 0.15` in `ScoringParams` default at `scoring.rs` line 59. No per-type abstention method exists. Plan's correction of the R1 review's incorrect `abstention_min() => Some(0.70)` claim is accurate.

10. **GRAPH_NEIGHBOR_FACTOR**: Confirmed `GRAPH_NEIGHBOR_FACTOR: f64 = 0.1` at `scoring.rs` line 193. Plan's "currently 0.1, test 0.15-0.2" is grounded.

11. **Importance scoring formula**: Confirmed `importance_floor + importance * importance_scale` = `0.3 + importance * 0.5` at `advanced.rs` lines 412-413. An importance of 0.5 gives factor 0.55 vs importance 0.3 giving 0.45 -- the R1 concern about 0.3 being "write-only" was slightly overstated (18% scoring penalty, not invisibility), but raising to 0.5 is a reasonable improvement.

12. **UserPreference has no dedup_threshold**: Confirmed at `domain.rs` lines 162-172. `UserPreference` is not in the `dedup_threshold()` match arms. It IS in `is_supersession_type()`, so near-duplicates get superseded but there's no store-time dedup gate. Plan correctly identifies this gap.

13. **retry_on_lock implementation**: Confirmed 5 max attempts, exponential backoff starting at 10ms, at `conn_pool.rs` lines 23-26 and 36-63. Plan's description of the mechanism is functionally correct (the range is imprecise but the behavior description is right).

14. **Effort estimates**: Post-R1 estimates (Wave 1: 2-3 days, Wave 2: 5-7 days, total 7-10 days) appear realistic given the scope, with the caveat from H1 that Wave 1 may need +1-2 days if `mag hook` CLI must be implemented.

---

## Edge Cases Examined

### Auto-capture hook silent failure
The plan notes hooks use `2>/dev/null || true` (Section 2C, line 127) and proposes telemetry logging (DG-M6). This is good, but there's a subtlety: if `mag hook` doesn't exist (see H1), the telemetry logging is also silenced. The fix must ensure that even failed hook invocations are logged -- the logging must happen in the shell script BEFORE the `mag` call, not as output from it.

### PostToolUse pattern matching misses
The plan acknowledges PostToolUse fires on every tool use (line 119) and must filter fast. Edge cases not addressed:
- `jj commit` vs `jj describe && jj new` -- different commit patterns produce different output formats
- Piped commands: `cargo test 2>&1 | tee log.txt` -- the Bash tool result may include the full pipe, not just cargo output
- Multiple git/jj invocations in a single Bash tool call (common in chained commands)

These are implementation details, not plan-level issues. The plan's 1.5-2.5 day estimate for PostToolUse hooks appropriately accounts for this complexity.

### Token budget interaction
The plan's three budget levels (welcome: 3,300, session-start hook: 2,000, compact-refresh: 800) operate on different code paths. There is no double-counting risk because:
- `welcome()` is the Rust function assembling data from SQLite
- `session-start.sh` calls `mag hook session-start` which would invoke `welcome()` internally
- `compact-refresh.sh` calls a different path for post-compaction re-injection

The 3,300 + 2,000 question is: does the hook output wrap the welcome output? If `mag hook session-start` internally calls `welcome()`, then the 2,000 hook budget should be the OUTER cap, and the 3,300 welcome budget is moot (the hook would truncate to 2,000 anyway). The plan should clarify the nesting relationship.

---

## Summary

| Severity | Count | Items |
|----------|-------|-------|
| CRITICAL | 0 | -- |
| HIGH | 1 | H1: `mag hook` CLI doesn't exist |
| MEDIUM | 3 | M1: TTL contradiction, M2: 9K token claim inflated, M3: retry backoff range |
| LOW | 2 | L1: Unmeasurable success metric, L2: Upgrade nudge delivery gap |
| Verified correct | 14 | See list above |

**Verdict:** The plan is structurally sound and the R1 revision was thorough. The single HIGH finding (H1) requires acknowledgment and a decision on approach before Phase 1 begins. The MEDIUM findings are precision corrections that prevent implementation confusion. No new CRITICAL issues were introduced by the R1 revision.
