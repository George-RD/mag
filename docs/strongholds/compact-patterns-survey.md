# Compact Hook Patterns Survey

**Status:** done
**Last updated:** 2026-04-07
**Updated by:** Uruk-hai scout

## Summary

Five distinct compact-related hook patterns found across 5 plugins. Two plugins use native PreCompact/PostCompact hook types (archived next-level, MAG). Three use the SessionStart matcher workaround (`"startup|resume|clear|compact"`). No plugin currently uses PostCompact stdout to inject `additionalContext` — all PostCompact hooks are pure side-effects. The active ide-of-sauron plugin has a PreCompact script (`preserve-context.mjs`) present in every version but never wired. The romega-memory MAG plugin has a PostCompact hook that calls `mag welcome` but outputs nothing to the model context window.

Cross-reference: `$REPO/docs/strongholds/pre-compact-hook-research.md` contains the hook API contract (stdin fields, stdout contract, exit codes).

---

## Findings

### 1. ide-of-sauron (mordor-forge) — Active: marketplaces live + v1.5.1 cached

**Pattern: SessionStart with `compact` matcher**

All 6 cached versions (1.2.0-1.5.1) and the live marketplaces copy use identical compact handling. Matcher: `"startup|resume|clear|compact"`. All scripts timeout: 5s, synchronous.

Files:
- `$HOME/.claude/plugins/cache/mordor-forge/ide-of-sauron/1.5.1/hooks/hooks.json:5`
- `$HOME/.claude/plugins/marketplaces/mordor-forge/ide-of-sauron/hooks/hooks.json:5`

Scripts that fire on compact resume (identical to cold start):
1. `inject-sauron.mjs` — outputs `additionalContext`: full Sauron Protocol (Eight Laws, force types)
2. `jj-gate.mjs` — outputs `additionalContext` warning if jj not present
3. `preflight.mjs` (v1.5.0+) — outputs `additionalContext` warning if gh/jq/python3 missing

**PreCompact script exists but is NEVER wired (all versions):**
- `$HOME/.claude/plugins/marketplaces/mordor-forge/ide-of-sauron/hooks/scripts/preserve-context.mjs`
- Present since v1.2.0, absent from every `hooks.json`. Dead code.
- Would output: `additionalContext` telling agent to write a context-snapshot stronghold.
- Note: Even if wired, PreCompact output is ignored per API contract (fire-and-forget only).

Anti-patterns:
- No PreCompact hook wired despite script existing
- Compact resume identical to cold start — no differentiated context
- Synchronous chain of 3 x 5s = up to 15s blocking on session start

---

### 2. archived next-level (claude-next-level) — Archived, NOT active

**Pattern: True PreCompact + SessionStart workaround for PostCompact**

File: `$HOME/.claude/plugins/marketplaces/claude-next-level/archived/next-level/hooks/hooks.json`

**PreCompact hook** (line 80-89, timeout: 5s):
- Script: `hooks/scripts/pre-compact.sh`
- Reads `session_id` and `transcript_path` from stdin JSON (correct pattern)
- Captures: active specs from specs dir, cwd, recent file from transcript, context percentage
- Writes: `$STATE_DIR/pre-compact-state.json` via `jq -n`
- Output: side-effect only, exit 0 — correct PreCompact contract
- Dependencies: jq, bash, utils.sh, compgen

**PostCompact restore — wired as SessionStart** (line 5-19, timeout: 5s):
- Script: `hooks/scripts/post-compact-restore.sh`
- Reads `pre-compact-state.json`; exits 0 silently if missing (correct guard)
- Deletes snapshot after reading (one-time-use pattern — correct)
- Output: `exit 2` with bare `{"result":$msg}` — NOT `additionalContext`; likely inert for context injection
- Dependencies: jq, bash, utils.sh

Anti-patterns:
- `exit 2` + bare `result` field is wrong output contract (should be `additionalContext` in JSON stdout)
- `eval "$jq_out"` on parsed JSON — security anti-pattern
- SessionStart fires for all trigger types; gates only via snapshot file existence (no compact matcher)

---

### 3. MAG plugin (mag-plugins) — Active: v0.1.1 cached / romega-memory plugin/

**Pattern: True PostCompact hook — side-effect only, NO additionalContext**

Files:
- `$HOME/.claude/plugins/cache/mag-plugins/mag/0.1.1/hooks/hooks.json:25-34`
- `$REPO/plugin/scripts/compact-refresh.sh`

PostCompact hook, timeout: 3000ms (tightest in survey).

Script behavior (`compact-refresh.sh`):
1. Writes log entry to `~/.mag/auto-capture.log` (line 11)
2. Calls: `mag welcome --project $PROJECT --session-id $SESSION_ID --budget-tokens 800` (line 13)
3. No stdout output — pure side-effect

**Critical gap:** `mag welcome` refreshes memories in the MAG store but the output is discarded. The model context window receives NOTHING after compaction. The prior research doc (`pre-compact-hook-research.md:77`) describes the intended behavior but the current code does not output `additionalContext`.

No PreCompact hook — nothing captures pre-compact state.

---

### 4. superpowers (claude-plugins-official) — Disabled in settings

**Pattern: SessionStart with compact matcher — no compact-specific logic**

File: `$HOME/.claude/plugins/cache/claude-plugins-official/superpowers/5.0.7/hooks/hooks.json:3-14`
Matcher: `"startup|clear|compact"` (no `resume`). No explicit timeout. Single command: `run-hook.cmd session-start`. No PreCompact or PostCompact hooks.

---

### 5. vercel plugin (claude-plugins-official) — Disabled in settings

**Pattern: SessionStart with compact matcher — no compact-specific logic**

File: `$HOME/.claude/plugins/cache/claude-plugins-official/vercel/7807a6aabad5/hooks/hooks.json:3-20`
Matcher: `"startup|resume|clear|compact"`. Three scripts: `session-start-seen-skills.mjs`, `session-start-profiler.mjs`, `inject-claude-md.mjs` (5s each). No compact-specific path. No PreCompact or PostCompact hooks.

---

## Settings-Level Hooks (`~/.claude/settings.json`)

No compact-related hooks. `$HOME/.claude/settings.json:121-169` has SessionStart/Stop/UserPromptSubmit/PostToolUse hooks for omega — no `compact` matcher, no PreCompact, no PostCompact. Compact events are invisible to omega.

---

## additionalContext Usage Matrix

| Plugin | Hook Type | Outputs additionalContext? | Notes |
|--------|-----------|---------------------------|-------|
| ide-of-sauron inject-sauron.mjs | SessionStart/compact | YES | Full Sauron Protocol on every compact resume |
| ide-of-sauron jj-gate.mjs | SessionStart/compact | YES (conditional) | Only if jj missing |
| ide-of-sauron preflight.mjs | SessionStart/compact | YES (conditional) | Only if tools missing |
| ide-of-sauron preserve-context.mjs | PreCompact (UNWIRED) | N/A — dead code | PreCompact output is ignored anyway |
| next-level pre-compact.sh | PreCompact | NO — side-effect | Correct: writes snapshot to disk |
| next-level post-compact-restore.sh | SessionStart (all) | NO — uses result field | Wrong field; likely inert |
| MAG compact-refresh.sh | PostCompact | NO — side-effect | mag welcome output discarded |
| superpowers run-hook.cmd | SessionStart/compact | Unknown | Not inspected |
| vercel inject-claude-md.mjs et al. | SessionStart/compact | YES | No compact-specific path |
| omega fast_hook.py | None | N/A | Compact events not handled |

---

## Key Observations

1. **No plugin uses PostCompact stdout `additionalContext`.** MAG fires the correct PostCompact hook but discards its output. Next-level uses bare `result` field (wrong; inert). Model context window receives nothing injected from PostCompact in any active plugin.

2. **ide-of-sauron is the only active plugin that reinjects context on compact resume** (SessionStart compact-matcher workaround). Content is identical to cold start — no compaction-specific state recovery.

3. **MAG has a critical gap:** PostCompact hook fires, `mag welcome` refreshes the store, but no `additionalContext` is emitted. Fix: capture `mag welcome` output and emit `{"additionalContext":"..."}` on stdout.

4. **ide-of-sauron `preserve-context.mjs` has been dead code since v1.2.0.** Even if wired as PreCompact, its intent is wrong — PreCompact output is fire-and-forget (ignored per API contract).

5. **Timeout summary (compact-path scripts):**
   - ide-of-sauron: 3 scripts x 5s synchronous = up to 15s blocking
   - next-level PreCompact: 5s; SessionStart restore: 5s
   - MAG PostCompact: 3s
   - omega: not on compact path

6. **Dependencies (compact-path only):**
   - ide-of-sauron: Node.js only
   - next-level: jq, bash, local utils.sh
   - MAG: mag CLI, sh
