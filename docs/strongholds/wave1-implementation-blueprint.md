# Wave 1 Implementation Blueprint

> Stronghold document. Concrete implementation plan for MAG Wave 1: Fix hook scripts + add auto-capture.
> Status: READY FOR IMPLEMENTATION
> Date: 2026-04-01
> Parent: `docs/strongholds/mag-improvement-plan.md` (Wave 1 section)
> DG reviews: `improvement-plan-dg-r1.md`, `improvement-plan-dg-r2.md`

---

## Summary

Wave 1 is pure shell + markdown work — no Rust required. All existing hook scripts are silent NOPs because they call `mag hook`, a CLI subcommand that does not exist (`src/cli.rs` `Commands` enum has no `Hook` variant). Skills and `plugin/CLAUDE.md` also direct the AI to call the same nonexistent command. This blueprint fixes all broken invocations and adds PostToolUse auto-capture for commits and build errors.

**Total estimated effort:** 2.5-3.5 days
**Rust changes:** None
**Benchmark gate required:** No (no Rust scoring/search changes)
**Quality gate:** `prek run` on any incidentally modified `.rs` files (none expected)

---

## Parallelization Map

```
Unit A (session scripts)   ──────────────────────────────────── merge
Unit B (commit-capture)    ────────────────── develop ──── merge B
Unit C (error-capture)     ────────────────── develop ────────── merge C (after B)
Unit D (skill rewrites)    ──────────────────────────────────── merge
```

Units A, B, C, D are developed simultaneously in separate jj workspaces.
B and C must merge in order (B first) because both modify `plugin/hooks/hooks.json`.
All other pairs are fully independent.

---

## Codebase Facts This Blueprint Depends On

These were verified against source before writing. Do not re-derive them.

| Fact | Source |
|------|--------|
| `mag hook` does not exist | `src/cli.rs` `Commands` enum — no `Hook` variant |
| `mag welcome --project P --session-id S` works | `src/cli.rs` line 321 |
| `mag process "content" --event-type T --project P --importance N` works | `src/cli.rs` line 102 |
| `mag advanced-search "query" --project P --limit N` works | `src/cli.rs` line 184 |
| `mag recent --limit N --project P` works | `src/cli.rs` line 221 |
| `SessionSummary` has TTL_EPHEMERAL = 3600s (1 hour) — destroyed before next session | `src/memory_core/domain.rs` line 114 |
| `SessionEnd` has TTL_LONG_TERM = 1,209,600s (14 days) — correct for session summaries | `src/memory_core/domain.rs` line 127 |
| `GitCommit` has TTL_LONG_TERM (14 days) | `src/memory_core/domain.rs` line 123 |
| `ErrorPattern` has TTL None (permanent) + type_weight 2.0 | `src/memory_core/domain.rs` lines 116, 147 |
| `hooks.json` has SessionStart, UserPromptSubmit, PostCompact, Stop — no PostToolUse | `plugin/hooks/hooks.json` |
| All three session scripts call `mag hook` and are NOPs | `plugin/scripts/session-start.sh:4`, `session-end.sh:5`, `compact-refresh.sh:3` |
| `plugin/CLAUDE.md` and skills reference `mag hook search` / `mag hook store` (broken) | `plugin/CLAUDE.md:17-18`, `plugin/skills/memory-recall/SKILL.md:14-16`, `plugin/skills/memory-store/SKILL.md:22` |

---

## Unit A: Session Lifecycle Scripts

**Scope:** Rewrite `session-start.sh`, `session-end.sh`, `compact-refresh.sh` to call existing CLI commands. Add telemetry logging to each.
**Effort:** 0.75 day
**Dependencies:** None
**jj bookmark:** `feat/wave1-session-scripts`

### `plugin/scripts/session-start.sh` — full replacement

```sh
#!/bin/sh
# MAG session start — recall project context
# Outputs memory context for Claude Code injection
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Log BEFORE invocation so failed mag calls are still recorded
mkdir -p "$HOME/.mag"
printf '%s session_start project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

mag welcome --project "$PROJECT" --session-id "$SESSION_ID" 2>/dev/null || true
```

### `plugin/scripts/session-end.sh` — full replacement

```sh
#!/bin/sh
# MAG session end — store lightweight session summary
# Stop hook: $CLAUDE_TRANSCRIPT may be set by Claude Code
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Build summary from transcript tail if available; fall back to minimal marker
if [ -n "${CLAUDE_TRANSCRIPT:-}" ]; then
  SUMMARY="Session ended. Project: $PROJECT. Recent context: $(printf '%s' "$CLAUDE_TRANSCRIPT" | tail -c 500)"
else
  SUMMARY="Session ended. Project: $PROJECT."
fi

# Log BEFORE invocation
mkdir -p "$HOME/.mag"
printf '%s session_end project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

# Use session_end event type (TTL_LONG_TERM = 14 days).
# Do NOT use session_summary: that maps to TTL_EPHEMERAL (1 hour) and self-destructs.
mag process "$SUMMARY" \
  --event-type session_end \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.4 2>/dev/null || true
```

### `plugin/scripts/compact-refresh.sh` — full replacement

```sh
#!/bin/sh
# MAG post-compact — re-inject top memories after context compaction
# Budget trimming deferred to Wave 2 (requires --budget-tokens flag, not yet in Rust)
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

mkdir -p "$HOME/.mag"
printf '%s compact_refresh project=%s session=%s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" >> "$LOG" 2>/dev/null || true

mag welcome --project "$PROJECT" --session-id "$SESSION_ID" 2>/dev/null || true
```

### Acceptance Criteria

- `mag welcome` output appears in Claude Code session context on session start and after compaction
- After session end, `mag search "session ended" --project P` in a new session returns a `session_end` memory
- `mag retrieve <id>` on that memory shows `ttl_seconds = 1209600` (14 days), not 3600
- `~/.mag/auto-capture.log` gets a line for each hook firing, including when `mag` is not installed
- `grep "mag hook" plugin/scripts/session-start.sh plugin/scripts/session-end.sh plugin/scripts/compact-refresh.sh` returns zero results

---

## Unit B: PostToolUse Commit-Capture Hook

**Scope:** Add `PostToolUse` event to `hooks.json`. Create `commit-capture.sh` that captures `jj commit`, `jj describe -m`, and `git commit -m` messages as `git_commit` memories.
**Effort:** 1-1.5 days
**Dependencies:** None (develop in parallel; merge after Unit A for clean history)
**jj bookmark:** `feat/wave1-commit-capture`

### `plugin/hooks/hooks.json` — add PostToolUse section

Append a `PostToolUse` key at the same level as the existing four hook types. Preserve all existing content verbatim.

### `plugin/scripts/commit-capture.sh` — new file

```sh
#!/bin/sh
# MAG commit-capture — auto-capture jj/git commit messages as Decision memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (JSON), $CLAUDE_TOOL_OUTPUT (JSON)
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Fast-path rejection — exit immediately if not a commit command
COMMAND="$(printf '%s' "${CLAUDE_TOOL_INPUT:-}" | jq -r '.command // empty' 2>/dev/null || true)"

case "$COMMAND" in
  *"jj commit"*|*"jj describe"*|*"git commit"*)
    : # fall through
    ;;
  *)
    exit 0
    ;;
esac

# Extract commit message from -m flag in the command string (most reliable)
MSG="$(printf '%s' "$COMMAND" | sed -n "s/.*-m[[:space:]]*[\"']\([^\"']*\)[\"'].*/\1/p" | head -1 || true)"

# Fallback: parse jj output for "Working copy now at: <hash> <message>"
if [ -z "$MSG" ]; then
  OUTPUT="$(printf '%s' "${CLAUDE_TOOL_OUTPUT:-}" | jq -r '.output // empty' 2>/dev/null || true)"
  MSG="$(printf '%s' "$OUTPUT" | sed -n 's/Working copy now at: [a-z0-9]* //p' | head -1 | head -c 200 || true)"
fi

# Bail if no message extracted
[ -n "$MSG" ] || exit 0

# Log BEFORE invocation
mkdir -p "$HOME/.mag"
printf '%s git_commit project=%s session=%s msg=%.80s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" "$MSG" >> "$LOG" 2>/dev/null || true

mag process "Commit: $MSG" \
  --event-type git_commit \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 2>/dev/null || true
```

### Acceptance Criteria

- After `jj commit -m "feat: test capture"`, `mag search "test capture" --project P` returns a `git_commit` memory with importance 0.5
- `mag retrieve <id>` shows `ttl_seconds = 1209600` (14 days)
- Bash calls without commit patterns complete the hook in under 50ms
- `~/.mag/auto-capture.log` has a `git_commit` line with message preview
- `jj new` alone (no message) produces no storage

---

## Unit C: PostToolUse Error-Capture Hook

**Scope:** Add `error-capture.sh` to the existing `PostToolUse` Bash hooks block (added by Unit B). Captures `cargo` and `npm` build/test failures as `error_pattern` memories.
**Effort:** 0.5-1 day
**Dependencies:** Unit B must be merged first (shared `hooks.json`)
**jj bookmark:** `feat/wave1-error-capture`

### `plugin/hooks/hooks.json` — extend existing PostToolUse block

After Unit B is merged, update the `PostToolUse` Bash hooks array to include a second entry for error-capture.sh.

### `plugin/scripts/error-capture.sh` — new file

```sh
#!/bin/sh
# MAG error-capture — auto-capture cargo/npm build/test failures as ErrorPattern memories
# PostToolUse(Bash) hook. MUST exit fast (<50ms) for non-matching commands.
# Receives: $CLAUDE_TOOL_INPUT (JSON), $CLAUDE_TOOL_OUTPUT (JSON)
set -eu

PROJECT="$(basename "$PWD")"
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
LOG="$HOME/.mag/auto-capture.log"

# Fast-path rejection — exit immediately if not a build/test command
COMMAND="$(printf '%s' "${CLAUDE_TOOL_INPUT:-}" | jq -r '.command // empty' 2>/dev/null || true)"

case "$COMMAND" in
  *"cargo test"*|*"cargo build"*|*"cargo check"*|*"cargo clippy"*|*"npm test"*|*"npm run"*|*"prek run"*)
    : # fall through
    ;;
  *)
    exit 0
    ;;
esac

# Extract output and check for failure signals
OUTPUT="$(printf '%s' "${CLAUDE_TOOL_OUTPUT:-}" | jq -r '.output // empty' 2>/dev/null || true)"

case "$OUTPUT" in
  *"FAILED"*|*"error["*|*"error: "*)
    : # fall through to store
    ;;
  *)
    exit 0  # passing run — do not store
    ;;
esac

# Extract first specific error line
ERROR_LINE="$(printf '%s' "$OUTPUT" | grep -m1 -E '^error(\[E[0-9]+\])?: ' | head -c 200 || true)"

# Fallback: first FAILED line
if [ -z "$ERROR_LINE" ]; then
  ERROR_LINE="$(printf '%s' "$OUTPUT" | grep -m1 'FAILED' | head -c 200 || true)"
fi

[ -n "$ERROR_LINE" ] || exit 0

# Log BEFORE invocation
mkdir -p "$HOME/.mag"
printf '%s error_pattern project=%s session=%s err=%.80s\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$PROJECT" "$SESSION_ID" "$ERROR_LINE" >> "$LOG" 2>/dev/null || true

mag process "Build/test error in $PROJECT: $ERROR_LINE" \
  --event-type error_pattern \
  --project "$PROJECT" \
  --session-id "$SESSION_ID" \
  --importance 0.5 2>/dev/null || true
```

### Acceptance Criteria

- After a failing `cargo test`, `mag search "error" --project P` returns an `error_pattern` memory with importance 0.5
- `mag retrieve <id>` shows no TTL (permanent)
- After a passing `cargo test`, no new `error_pattern` memory is stored
- Non-build Bash commands complete the hook in under 50ms

---

## Unit D: Skill and CLAUDE.md Rewrites

**Scope:** Replace all `mag hook` references in plugin-facing documentation and AI-instruction files with actual working CLI commands.
**Effort:** 0.5 day
**Dependencies:** None
**jj bookmark:** `feat/wave1-skill-rewrites`

### Files to Modify

- `plugin/CLAUDE.md` — replace `mag hook search`/`mag hook store` with `mag advanced-search`/`mag process`
- `plugin/skills/memory-recall/SKILL.md` — replace `mag hook search` examples with `mag advanced-search` / `mag recent`
- `plugin/skills/memory-store/SKILL.md` — replace `mag hook store` with `mag process`
- `plugin/scripts/prompt-gate.sh` — replace `mag memory_store`/`mag memory_search` hints with correct CLI commands

### Acceptance Criteria

- `grep -r "mag hook" plugin/` returns zero results
- `grep -r "mag advanced-search\|mag process\|mag recent" plugin/` returns results in all modified files
- Skill invocations work end-to-end

---

## Merge Order

1. Unit A (`feat/wave1-session-scripts`) — merge immediately
2. Unit D (`feat/wave1-skill-rewrites`) — merge immediately (independent)
3. Unit B (`feat/wave1-commit-capture`) — merge (adds PostToolUse to hooks.json)
4. Unit C (`feat/wave1-error-capture`) — rebase on B, then merge

---

## Success Criteria (Wave 1 Complete)

| Check | Verification |
|-------|-------------|
| No broken `mag hook` calls in plugin | `grep -r "mag hook" plugin/` returns zero results |
| Session start injects context | Start Claude Code session; `mag welcome` output visible |
| Session end stores 14-day memory | `mag search "session ended"` returns result in new session |
| Commit capture works | `jj commit -m "test"` then `mag search "test"` returns `git_commit` |
| Error capture works | Failing `cargo test` then `mag search "error"` returns `error_pattern` |
| Telemetry logging | `cat ~/.mag/auto-capture.log` shows all event types |
| Quality gate passes | `prek run` passes |
