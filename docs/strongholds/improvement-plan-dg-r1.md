# Adversarial Review: MAG Improvement Plan

> Reviewer: Devil's Advocate (DG Round 1)
> Date: 2026-03-31
> Document under review: `docs/strongholds/mag-improvement-plan.md`
> Sources: Full codebase survey, AGENTS.md, mcp_server.rs, scoring.rs, plugin hooks, session scripts

---

## Overall Verdict: REVISE

The plan is well-structured and grounded in real codebase knowledge. The three-pillar framework is sound and the "deferred" list shows discipline. However, there are several factual inaccuracies about current state, a dangerous assumption about auto-capture that contradicts the codebase's own design, an underestimation of Wave 2/3 effort, and a critical user-experience blind spot around the MCP collapse. The plan should be revised to address the findings below before implementation begins.

---

## Findings

### CRITICAL

#### C1. Plan claims "cosine as primary signal, Jaccard as secondary" is a FIX -- but the codebase already does this (Confidence: 95)

**Evidence:** `src/memory_core/storage/sqlite/crud.rs` lines 197-216 and `src/memory_core/storage/sqlite/mod.rs` lines 53-60 show that supersession already uses cosine as the primary gate (`SUPERSESSION_COSINE_THRESHOLD = 0.70`), with Jaccard as the secondary confirmation (`SUPERSESSION_JACCARD_THRESHOLD = 0.30`). The code checks cosine first and `continue`s if it fails, then checks Jaccard.

The plan states in Section 3A: "Supersession fix: Cosine as primary signal, Jaccard as secondary (catches 9/10 more KU pairs)." This is literally the current implementation. Either the plan is describing already-shipped work as future work, or the author means something different (e.g., lowering thresholds, removing Jaccard entirely) but hasn't specified what.

**Risk:** If this is double-counted work, Wave 2 loses a deliverable and the "scoring refinements" pillar is thinner than presented. If the author means threshold tuning, the lack of specific target values makes this unimplementable.

**Recommended fix:** Either remove this item and acknowledge it's already done, or specify exact threshold changes with benchmark evidence for the "9/10 more KU pairs" claim.

---

#### C2. SessionSummary has TTL_EPHEMERAL (1 hour) -- auto-capture at importance 0.4 will create ghosts (Confidence: 92)

**Evidence:** `src/memory_core/domain.rs` line 7: `TTL_EPHEMERAL = 3600` (1 hour). Line 114: `SessionSummary => Some(TTL_EPHEMERAL)`. The plan proposes auto-capturing session summaries as `SessionSummary` with importance 0.4.

These memories will self-destruct after 1 hour. The sweep job will delete them before they can ever be recalled in a subsequent session. This makes the entire "session summary auto-capture" feature useless -- the user will never see the benefit because the data won't survive between sessions.

**Risk:** The crown jewel of Pillar 2 ("install MAG and benefit naturally") depends on session summaries persisting across sessions. With a 1-hour TTL, the user gets zero value from this feature.

**Recommended fix:** Either (a) override TTL to `None` or `TTL_LONG_TERM` for auto-captured session summaries, (b) use a different event type like `Decision` or `LessonLearned` that has longer TTL, or (c) add TTL override to the auto-capture hook. This needs to be explicitly addressed in the plan because it contradicts the existing type system's design intent.

---

#### C3. PostToolUse hooks do not exist in the plugin -- plan claims they're "no Rust" work (Confidence: 91)

**Evidence:** `plugin/hooks/hooks.json` defines only four hook events: `SessionStart`, `UserPromptSubmit`, `PostCompact`, and `Stop`. There is no `PostToolUse` entry. Grepping the entire plugin directory for "PostToolUse" returns zero results. The plan lists PostToolUse hooks for commit capture and error capture as "Layer 1 (hooks/skills only, no Rust)" work.

While Claude Code does support PostToolUse as a hook event type, MAG's plugin has never used it. The plan treats this as trivially adding shell scripts, but PostToolUse hooks have significant complexity:
- They fire on every tool use (Bash, Read, Write, Edit, etc.), requiring fast pattern matching to filter relevant calls
- The 50ms timeout used for `UserPromptSubmit` in `hooks.json` shows the plugin already has timeout sensitivity
- Capturing commit descriptions requires parsing jj/git output from Bash tool results
- Capturing cargo test failures requires parsing stderr output and extracting meaningful patterns

This is materially more complex than "add a shell script." The effort estimate of "1 day (shell scripts only)" for all of Wave 1 is unrealistic when PostToolUse pattern matching and output parsing are included.

**Recommended fix:** Break PostToolUse hooks into their own mini-wave with proper effort estimate (1-2 days for both). Specify the matcher patterns, timeout values, and output parsing strategy. Acknowledge this is new territory for the plugin.

---

### HIGH

#### H1. MCP tool collapse from 16 to 4 will break every existing MCP integration simultaneously (Confidence: 88)

**Evidence:** `mcp_server.rs` uses the `rmcp` crate's `#[tool(...)]` macro system. Each of the 16 tools has its own Rust struct for parameters with `#[derive(JsonSchema)]`. The plan proposes a `--mcp-tools=minimal|full` flag defaulting to `full`, but the MCP instructions (lines 54-90 of `mcp_server.rs`) already reference the current 16-tool names. Any MCP client that has cached tool schemas or has hardcoded tool names will break when switching to 4-tool mode.

More critically: the plan doesn't address how the 4 new tools will handle parameter disambiguation. When `memory` absorbs `store`, `search`, `retrieve`, and `delete`, the AI client must decide which action to take. This means the new `memory` tool needs an `action` parameter, and the AI must learn the routing semantics. There is no evidence that major MCP clients (Claude Desktop, Cursor, etc.) handle action-routing tools as well as they handle purpose-specific tools.

**Risk:** The token savings (2-3K to ~500) are real, but you may trade token efficiency for accuracy. A unified `memory` tool with 5 sub-actions has higher cognitive load for the AI client than 5 separate tools with clear names. The plan's own precedent -- `memory_lifecycle` already uses `action` routing -- should be evaluated for how well it works in practice before scaling this pattern.

**Recommended fix:** Add a validation step: deploy the 4-tool facade to a single MCP client (Claude Desktop) with A/B testing against the 16-tool mode. Measure task completion rate, not just token count. If the AI makes more routing errors with the collapsed tools, the token savings are a net negative.

---

#### H2. welcome() currently has no token budget enforcement -- plan assumes it exists or is trivial (Confidence: 87)

**Evidence:** `src/memory_core/storage/sqlite/admin.rs` lines 888-985 show the welcome() implementation. It fetches up to 15 recent memories (truncated to 200 chars each) and up to 20 user preferences (truncated to 300 chars each). There is no token counting or budget cap logic anywhere in this function. The `--budget-tokens` flag appears only in the CLI hook scripts (`session-start.sh` passes `--budget-tokens 2000`, `compact-refresh.sh` passes `--budget-tokens 800`), but these likely map to a different code path (the `mag hook` CLI subcommand) and not to the welcome() trait method itself.

The plan proposes a "3,300 token budget cap" for welcome in Wave 3 but treats it as a simple constant. Implementing token budgeting requires: (a) a token counting mechanism (char-based approximation or actual tokenizer), (b) priority ordering to decide what gets cut, (c) the 4-tier injection hierarchy which doesn't exist yet. This is not a config change; it's new logic in the welcome path.

**Recommended fix:** Size token budget enforcement as 1-2 days of work on its own. Specify the token counting strategy (chars/4 approximation is fine for MVP). Acknowledge that the 4-tier hierarchy must land first or simultaneously.

---

#### H3. welcome() trait signature has no agent_type or entity_id parameters -- Wave 3 requires a breaking trait change (Confidence: 86)

**Evidence:** `src/memory_core/traits.rs` lines 243-250:
```rust
pub trait WelcomeProvider: Send + Sync {
    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value>;
}
```

The plan states "welcome() extends to accept agent_type + entity_id params" as Wave 3 work. This is a trait signature change. Per AGENTS.md conventions: "Trait-first design -- add new trait + impl rather than modifying existing signatures." Modifying `WelcomeProvider::welcome()` violates the project's own conventions and breaks every existing implementor and caller.

**Risk:** This is either a convention violation (changing existing trait) or requires creating a new trait (e.g., `ScopedWelcomeProvider`) which the plan doesn't account for.

**Recommended fix:** Either (a) use a `WelcomeOptions` struct parameter (following the `SearchOptions` pattern already established in the codebase) to add fields without breaking the signature, or (b) create a new trait per conventions. Update the plan to reflect the chosen approach and the additional effort.

---

#### H4. "Single SQLite DB is fine for multi-agent" assumption is untested and hand-waved (Confidence: 84)

**Evidence:** The plan's Assumption #2 states: "Single SQLite DB is fine for multi-agent -- isolation via filtering, not sharding." But the current codebase uses a writer mutex (`pool.writer()`) for all write operations. Multi-agent means multiple concurrent AI tools storing memories simultaneously. SQLite's write-ahead log can handle concurrent reads, but concurrent writes from multiple `mag` processes will contend on the single writer lock.

The `retry_on_lock()` utility (mentioned in AGENTS.md) uses bounded backoff with 5 attempts. Under multi-agent write load, this will either: (a) succeed with high latency spikes, or (b) fail after 5 retries and surface errors to users. The plan proposes no load testing or concurrency testing for multi-agent scenarios.

**Recommended fix:** Add a concurrency stress test to Wave 3: spawn N agents writing simultaneously, measure p99 latency and error rate. If retries exceed 2% of writes, the single-DB assumption needs revisiting. This is cheap to test and expensive to fix later.

---

#### H5. No rollback plan for schema migrations (Confidence: 82)

**Evidence:** AGENTS.md states: "Schema migrations additive only -- never drop/rename columns." The plan proposes adding `preference_level`, `last_confirmed_at`, and `agent_type` index in Wave 3. While additive-only is safe for forward compatibility, the plan has no rollback strategy if the new columns cause performance regression (e.g., wider rows slowing full-table scans) or if the index on `agent_type` bloats the database.

The plan's risk registry says "Additive only (never drop/rename columns per AGENTS.md)" but this is a design constraint, not a mitigation. What happens if `preference_level` filtering logic has a bug that corrupts welcome() output? You can't drop the column. The data is permanent.

**Recommended fix:** Add a migration validation step: run the full benchmark suite against a DB with the new schema and realistic data volumes (10K+ memories). Compare query latency before and after. Also: consider making `preference_level` a metadata JSON field rather than a column, since the plan already suggests "or use importance threshold" as an alternative.

---

### MEDIUM

#### M1. Cross-tool Tier 1 status table is misleading -- "Done" means MCP config, not hooks (Confidence: 78)

**Evidence:** The plan's cross-tool table (Section 3C) claims Tier 1 (Cursor, Windsurf, Cline, VS Code, Zed, Claude Desktop) is "Done" with "None" work needed. But the plugin hooks system (`hooks.json`, session-start/end scripts) only works in Claude Code. The other Tier 1 tools get MCP-only mode, which the plan itself identifies as degraded ("MCP-only mode is degraded: no hooks = no automatic recall/store").

If the core thesis is "making MAG useful without asking the user to do anything," then Tier 1 tools other than Claude Code are not "Done" -- they're in the degraded state the plan is trying to fix.

**Recommended fix:** Split the status column into "MCP Config" and "Auto-capture" columns. Only Claude Code should show "Done" for auto-capture. This changes the competitive picture significantly.

---

#### M2. Error-capture hook stores ErrorPattern at importance 0.3 -- this will be invisible (Confidence: 76)

**Evidence:** `src/memory_core/domain.rs` shows `ErrorPattern` has `default_ttl() => None` (no expiry, good) but `type_weight() => 0.90` and `abstention_min() => Some(0.70)`. The plan proposes storing auto-captured errors at importance 0.3. Combined with the low importance and the reranker's natural demotion (as the plan itself notes), these memories will rank below virtually everything else. The abstention gate at 0.70 means they'll only surface if the query is extremely specific.

**Risk:** This is not "auto-capture that surfaces when relevant" -- this is auto-capture that's effectively write-only. The user will never see these memories recalled unless they search explicitly.

**Recommended fix:** Either raise auto-captured error importance to 0.5-0.6 (on par with commit descriptions), or reconsider whether auto-captured errors at importance 0.3 justify the storage and processing cost.

---

#### M3. Plan ignores the existing compact-refresh.sh budget mismatch (Confidence: 75)

**Evidence:** `session-start.sh` uses `--budget-tokens 2000`. `compact-refresh.sh` uses `--budget-tokens 800`. The plan proposes a welcome() token budget of 3,300 tokens. These three numbers are inconsistent. After compaction, the user gets only 800 tokens of context re-injected -- less than half of session start. If the plan's 3,300 budget is for the internal welcome() function, the hook scripts will independently cap at lower values. The plan doesn't reconcile these.

**Recommended fix:** Define the token budget hierarchy explicitly: welcome() internal cap, session-start hook budget, compact-refresh hook budget. Explain why compact-refresh should be lower and what the tradeoffs are.

---

#### M4. Effort estimate for Wave 2 MCP facade is unrealistic (Confidence: 79)

**Evidence:** `mcp_server.rs` is 1,689 lines with 16 tool handler functions, each with its own parameter struct, validation, and error handling. The plan proposes "New `mcp_router.rs` facade module routing unified tool names to existing handlers" in "1-2 days Rust." This requires:
- 4 new parameter structs (each a union of the absorbed tools' parameters)
- An `action` discriminator for each unified tool
- Input validation that varies by action
- Error messages that reference the correct sub-operation
- Schema generation that accurately describes the union type
- Tests for all routing paths (at minimum 16 positive tests + edge cases)
- The `--mcp-tools` CLI flag with conditional tool registration in rmcp

The rmcp crate uses `#[tool(...)]` proc macros. A facade that routes to existing handlers while maintaining the macro-generated schema is not a thin wrapper -- it requires understanding rmcp internals.

**Recommended fix:** Estimate Wave 2 MCP work as 3-4 days, not 1-2. Consider prototyping the `memory` tool first (absorbing store/search/retrieve/delete) as a spike to validate the routing pattern with rmcp before committing to all 4.

---

#### M5. "memorybench adoption" is listed but has no feasibility assessment (Confidence: 72)

**Evidence:** The plan mentions `supermemoryai/memorybench` for competitive comparison but provides no detail on: (a) whether the benchmark's format is compatible with MAG's input/output, (b) what metrics it uses, (c) whether the "apples-to-apples" comparison claim holds when MAG is fully local and competitors use LLM calls. If memorybench requires online LLM calls for evaluation, it breaks MAG's "deterministic word-overlap on full 1,986 questions" strength.

**Recommended fix:** Spend 2 hours evaluating memorybench's format and metrics before listing it as a deliverable. If it requires LLM judges, note this explicitly and the cost implications.

---

#### M6. No observability or telemetry plan for auto-capture (Confidence: 74)

**Evidence:** The success metric "Manual store_memory calls needed: ~5-10/session -> ~1-2/session" requires "Plugin telemetry" as its measurement method. But the plan includes no telemetry implementation work. The plugin hooks are shell scripts that run with `2>/dev/null || true` (both session-start.sh and session-end.sh), silently swallowing all errors and output. There is no mechanism to count auto-captured memories vs manual stores.

**Recommended fix:** Add a lightweight telemetry work item to Wave 1: have hooks log auto-capture events to a local file (`~/.mag/auto-capture.log`) that can be analyzed. Without this, the plan's success metrics are unmeasurable.

---

### LOW

#### L1. Dedup threshold for UserPreference (Jaccard >= 0.75) may be too aggressive (Confidence: 68)

The plan proposes Jaccard >= 0.75 as the dedup threshold for UserPreference memories. But preferences can be semantically identical with very different wording ("use tabs" vs "indent with tabs, not spaces"). Jaccard on 3-grams (the codebase's default) will miss these. This may cause preference accumulation that the dedup was meant to prevent.

**Recommended fix:** Consider cosine similarity (already computed during storage) as the primary dedup signal for preferences, matching the supersession approach.

---

#### L2. Plan doesn't mention Windows/Linux support implications (Confidence: 65)

All plugin hooks are POSIX shell scripts (`#!/bin/sh`). The plan proposes adding more shell hooks for PostToolUse. Windows users running Claude Code or Cursor will get no auto-capture functionality. The plan's Assumption #5 ("George is sole developer") suggests this is acceptable for now, but it should be noted as a known gap.

---

#### L3. "Codex CLI skill dirs exist" assumption is unverified and fragile (Confidence: 64)

The plan lists Assumption #4: "Codex CLI skill dirs exist -- verify `~/.codex/skills/` path before building." The codebase already has Codex detection in `src/tool_detection.rs` (checking `~/.codex/config.toml`), but skill directory support (`install_skills()`) is listed as future work. Codex is still evolving rapidly and directory structures may change. Building installer code against an unverified path risks immediate breakage.

---

## Summary of Recommendations

| Priority | Action |
|----------|--------|
| CRITICAL | Remove or respec supersession "fix" -- it's already implemented |
| CRITICAL | Fix SessionSummary TTL (1 hour) before auto-capture is useful |
| CRITICAL | Properly scope PostToolUse hook work -- it's not trivial |
| HIGH | Add A/B validation for MCP 4-tool collapse before committing |
| HIGH | Size token budget enforcement realistically (1-2 days) |
| HIGH | Address WelcomeProvider trait change per project conventions |
| HIGH | Add concurrency stress test for multi-agent SQLite |
| HIGH | Add migration validation benchmark |
| MEDIUM | Fix cross-tool status table to reflect hook vs MCP distinction |
| MEDIUM | Reconsider ErrorPattern importance 0.3 (effectively invisible) |
| MEDIUM | Reconcile token budget hierarchy across hooks and welcome() |
| MEDIUM | Re-estimate MCP facade effort to 3-4 days |
| MEDIUM | Evaluate memorybench compatibility before listing as deliverable |
| MEDIUM | Add basic auto-capture telemetry to make success metrics measurable |

## Net Assessment

The strategic direction is correct: MAG's retrieval is strong and the bottleneck is indeed storage friction. The three pillars are well-chosen. But the plan has several factual errors about codebase state that suggest it was written from memory notes rather than verified against code. The effort estimates are optimistic by roughly 40-60%, which is a significant risk for a solo developer. Most critically, the auto-capture crown jewel (session summaries) will silently fail due to TTL_EPHEMERAL unless explicitly addressed.

Fix the three critical items, re-estimate effort, and this plan is ready to execute.
