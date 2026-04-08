# JSONL Telemetry Campaign

**Status:** in-progress
**Last updated:** 2026-04-08
**Updated by:** George-RD

> Stronghold document. Campaign to upgrade MAG's `~/.mag/auto-capture.log` to structured JSONL,
> fix hook stdin parsing bugs, and add new capture capabilities.
> Parent: `docs/strongholds/mag-improvement-plan.md` (Wave 4: ATIC)

---

## 1. Campaign Overview

MAG's hook system captures session lifecycle events to `~/.mag/auto-capture.log`. The current format is ad-hoc plain text — each script uses a different field ordering, quoting style, and naming convention. There is no machine-readable structure, no schema version, and no way to filter by event type, session, or project without fragile grep patterns.

This campaign delivers three things:

1. **Structured JSONL output** — every hook script emits newline-delimited JSON to `~/.mag/auto-capture.jsonl` using a versioned schema.
2. **Hook bug fixes** — `session-start.sh` and `session-end.sh` don't read stdin at all; `prompt-gate.sh` reads raw text when stdin is JSON; `session-end.sh` references dead env vars.
3. **New capture capabilities** — `SubagentStop` handler to record delegated work outcomes; duration and error fields in every JSONL entry.

The JSONL log is the foundation for a future dashboard, batch graph-building, and cross-agent attribution. Get the data flowing first.

---

## 2. Current State (Intel Summary)

**Hook scripts:** 7 total in `plugin/scripts/`. 6 write to `auto-capture.log` (all except `prompt-gate.sh`). 2 properly parse stdin JSON (`pre-compact.sh`, `compact-refresh.sh`). The remaining 5 use env vars or raw `read`.

| Script | Stdin handling | Bugs |
|--------|---------------|------|
| `session-start.sh` | None — uses `$CLAUDE_SESSION_ID` env var | session_id always "unknown" if env not populated by Claude Code |
| `session-end.sh` | None — uses `$CLAUDE_SESSION_ID` and `$CLAUDE_TRANSCRIPT` env vars | `$CLAUDE_TRANSCRIPT` is dead code — Claude Code sends `transcript_path` and `last_assistant_message` in stdin JSON, not env vars |
| `prompt-gate.sh` | `read -r PROMPT` reads raw text | stdin is JSON; pattern matching against raw JSON string causes false negatives |
| `error-capture.sh` | Uses `$CLAUDE_TOOL_INPUT` / `$CLAUDE_TOOL_OUTPUT` env vars | `[error-capture]` bracket in log printf — inconsistent with all other scripts; no session= field |
| `commit-capture.sh` | Uses `$CLAUDE_TOOL_INPUT` / `$CLAUDE_TOOL_OUTPUT` env vars | None critical |
| `pre-compact.sh` | `INPUT=$(cat)` + jq | Correct |
| `compact-refresh.sh` | `INPUT=$(cat)` + jq | Correct |

**hooks.json gaps:**
- `Stop` hook uses `"*"` matcher, which fires for both main session stops and subagent stops via the plugin system. There is no `SubagentStop` handler registered.
- Result: every subagent completion fires `session-end.sh`, which logs a `session_end` entry. This inflates the log — 405 of 453 entries (89%) are `session_end`, the majority from subagent stops.

**Reranker status:** Cross-encoder reranker is implemented and works but is opt-in via `--cross-encoder` flag. The plugin's `mag serve` invocation does not pass this flag. The 87MB model is already downloaded. Enabling by default would improve recall quality at the cost of slightly higher latency per query.

**Codebase state:** Clean post-refactor (Wave 1-2 complete). A larger refactor is planned but targets Rust source, not plugin shell scripts. No merge conflict risk.

**Test harness approach:** `claude -p` with `--model haiku`, `--include-hook-events`, `--output-format stream-json`. Diff `mag list --json` before and after to verify memory capture. Scripts live in `tests/hooks/`.

---

## 3. Decision Log

| ID | Decision | Options | Chosen | Rationale | Status | Debate Notes |
|----|----------|---------|--------|-----------|--------|--------------|
| D1 | Should JSONL work happen before or after a planned major refactor? | Before / After / Concurrent | Before | JSONL work lives in `plugin/scripts/` (shell) and `plugin/hooks/hooks.json` — completely separate from Rust source the refactor will touch. Structured telemetry in place before a big refactor provides observability into system behavior during and after. No merge conflict risk since the layers don't overlap. | **ratified** | Schema should be v0 not v1 until context typing and duration_ms are resolved |
| D2 | Enable reranker by default in plugin? | Opt-in (current) / Default-on / Config flag | Deferred | Both Sauron and Saruman agree: benchmark needed before decision. Need latency delta on real hardware. | **deferred** | File issue with benchmark protocol |
| D3 | Add SubagentStop handler? | Yes / No | Yes (conditional) | Captures delegated work outcomes. Currently invisible — subagent completions fire the main `Stop` handler, creating log noise rather than signal. A dedicated `SubagentStop` handler with `subagent_end` event type gives clean attribution. | **ratified** | Stop and SubagentStop are mutually exclusive — confirmed in Claude Code docs. System auto-converts Stop → SubagentStop for subagent completions. No double-fire risk. Adding SubagentStop handler is safe. |
| D4 | How to handle subagent session_end log inflation? | Keep as-is / Deduplicate / Separate event type | Use `event: "hook.subagent_end"` with importance 0.3 (raised from 0.2) | Distinguishes main session endings from subagent stops without discarding the data. Downstream filtering via `jq 'select(.event == "hook.session_end")'` then works correctly. | **ratified** | Importance raised from 0.2 to 0.3. Hook still can't distinguish signal from noise — explicit mag process calls remain the primary mechanism for valuable memories. |
| D5 | Test harness approach? | Manual / claude -p / Unit tests | `claude -p` with enhanced validation | End-to-end: exercises the full hook pipeline including Claude Code's hook dispatch. Haiku is fast and cheap. `--include-hook-events` in stream-json output lets us assert hooks fired. | **ratified with gaps** | Add JSONL field validation (parse entries, assert required fields). Document compaction hook gap — claude -p cannot trigger PreCompact/PostCompact. Single-turn only limitation acknowledged. |
| D6 | Dev plugin isolation? | Modify production / ~/.dev-mag/ / Docker | Separate `~/.dev-mag/` with MAG_DATA_ROOT hardcoded in every script | Zero risk to live memories. Easy to reset. Can clone production DB for realistic testing without poisoning it. Parallel `plugin/dev/` variant with modified `.mcp.json`. | **ratified** | Dev scripts have MAG_DATA_ROOT hardcoded at line 6-7 of every script. Production scripts do NOT — separate concern for production migration. |
| D7 | Stale stronghold docs? | Leave / Delete / Archive | Archive with status header | Preserves history, stops confusion about whether docs are current targets. One-line header change at top of file. | **ratified** | Low stakes. Do it and move on. |

---

## 3a. Palantir Debate — Open Issues

1. **Schema version: v0 not v1** — Schema has freeform context block, hardcoded tool enum, and broken duration_ms on macOS. Not stable enough for v1. Use v0 until resolved.
2. **duration_ms broken on macOS** — `date +%s%N` not supported on BSD date (Darwin). Options: (a) use `date +%s` and rename to `duration_s`, (b) use gdate with platform check, (c) use perl one-liner `perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'`. Recommendation: option (c) for ms precision with POSIX fallback.
3. **project identifier fragility** — `basename "$PWD"` produces different values for forks, clones, worktrees. Consider `.mag-project` file, git remote URL, or `jj workspace root` basename. Lower priority — can ship with basename and improve later.
4. **Log rotation** — No mechanism planned. Add size-based rotation (50MB, keep 3) or document as known limitation for v0.
5. **Stop/SubagentStop exclusivity** — RESOLVED: Confirmed mutually exclusive. Safe to implement.
6. **Canonical example contradicts design principle** — Example shows null fields, but principle 2 says "null fields omitted in practice." Resolve: keep nulls in canonical example for documentation, omit in actual output.

---

## 4. JSONL Schema (v0)

### Canonical example

```jsonl
{"v":0,"ts":"2026-04-08T12:00:00Z","event":"hook.session_start","session_id":"abc123","project":"romega-memory","agent":{"id":null,"type":null,"tool":"claude_code"},"hook":{"name":"session-start","duration_ms":42,"status":"ok","error":null},"memory":null,"context":{}}
```

### Event taxonomy (`event` field)

**Hook lifecycle**

| Event | Fired by |
|-------|---------|
| `hook.session_start` | `session-start.sh` (SessionStart) |
| `hook.session_end` | `session-end.sh` (Stop — main session) |
| `hook.subagent_end` | `session-end.sh` or new handler (SubagentStop) |
| `hook.prompt_gate` | `prompt-gate.sh` (UserPromptSubmit) — only when hint emitted |
| `hook.pre_compact` | `pre-compact.sh` (PreCompact) |
| `hook.post_compact` | `compact-refresh.sh` (PostCompact) |
| `hook.commit_capture` | `commit-capture.sh` (PostToolUse/Bash) |
| `hook.error_capture` | `error-capture.sh` (PostToolUse/Bash) |

**Memory operations**

| Event | Meaning |
|-------|---------|
| `memory.store` | A memory was written via `mag process` |
| `memory.recall` | A memory was retrieved via `mag welcome` or `mag advanced-search` |
| `memory.update` | A memory was modified |
| `memory.delete` | A memory was deleted |
| `memory.consolidate` | Consolidation pass ran |
| `memory.compact` | Compact/summarisation ran |

**Hook errors**

| Event | Meaning |
|-------|---------|
| `hook.error` | Any hook script exited non-zero or timed out |

**System**

| Event | Meaning |
|-------|---------|
| `system.startup` | Daemon started |
| `system.shutdown` | Daemon stopped |
| `system.health_check` | Health probe result |

### Field reference

| Field | Type | Description |
|-------|------|-------------|
| `v` | integer | Schema version. `0` for this draft release. |
| `ts` | string | ISO 8601 UTC timestamp (`2026-04-08T12:00:00Z`). |
| `event` | string | Dotted event name from taxonomy above. |
| `session_id` | string\|null | From stdin JSON payload. `null` if not available. |
| `project` | string | Project name derived from `cwd` basename. |
| `agent.id` | string\|null | Subagent ID if applicable; `null` for main session. |
| `agent.type` | string\|null | Subagent type (`Explore`, `code-reviewer`, etc.); `null` for main session. |
| `agent.tool` | string | Coding tool that fired the hook. One of: `claude_code`, `claude_desktop`, `cursor`, `vscode_copilot`, `windsurf`, `cline`, `zed`, `codex`, `gemini_cli`, `opencode`. |
| `hook.name` | string | Which hook script fired (e.g. `session-start`). |
| `hook.duration_ms` | integer | Wall-clock time the hook script took in milliseconds. |
| `hook.status` | string | One of: `ok`, `error`, `timeout`, `skipped`. |
| `hook.error` | string\|null | Error message if `hook.status != "ok"`. |
| `memory.action` | string\|null | `store`, `recall`, `update`, or `delete`. Null if no memory op. |
| `memory.memory_id` | string\|null | UUID of the affected memory record. |
| `memory.event_type` | string\|null | One of MAG's 22 event types (e.g. `session_end`, `git_commit`, `error_pattern`). |
| `memory.importance` | float\|null | Importance score 0.0-1.0. |
| `memory.content_preview` | string\|null | First 80 characters of memory content. |
| `context` | object | Hook-specific freeform payload. Examples below. |

### `context` payloads by hook

| Hook | Typical `context` keys |
|------|----------------------|
| `session-start` | `cwd`, `trigger` |
| `session-end` | `last_assistant_message` (first 200 chars), `transcript_path` |
| `prompt-gate` | `prompt_preview` (first 80 chars), `hint_type` |
| `pre-compact` | `transcript_path`, `trigger`, `vcs_state`, `recent_file` |
| `post-compact` | `compact_summary` (first 200 chars) |
| `commit-capture` | `commit_message`, `vcs_tool` (`jj` or `git`) |
| `error-capture` | `error_line`, `command_preview` |
| `subagent-end` | `agent_id`, `agent_type` |

### Design principles

1. `v` field enables forward-compatible schema evolution — parsers can skip unknown versions.
2. Null fields are omitted in practice — scripts only include blocks relevant to the event.
3. `agent` block distinguishes main session events (`agent.type: null`) from subagent events, enabling clean attribution queries.
4. `hook` block captures performance and errors on every entry — timing regressions become visible.
5. `memory` block is populated only when a `mag process` or `mag advanced-search` call occurs within the hook.
6. `context` is intentionally freeform — hook-specific without bloating the top-level schema.
7. Compatible with `jq` filtering, a future web dashboard, and batch graph-building in dream-state.

---

## 5. Hook Fix Inventory

| Script | Fix | Priority |
|--------|-----|----------|
| `session-start.sh` | Read stdin JSON via `cat` + `jq` for `session_id`, `cwd`, and `hook_event_name`. Remove `$CLAUDE_SESSION_ID` env var fallback as primary source. | P0 |
| `session-end.sh` | Read stdin JSON for `session_id`, `last_assistant_message`, `transcript_path`, `hook_event_name`. Remove dead `$CLAUDE_TRANSCRIPT` env var reference. Use `last_assistant_message` from stdin as the session summary instead. | P0 |
| `prompt-gate.sh` | Replace `read -r PROMPT` with `INPUT=$(cat)` + `jq -r '.prompt // empty'` to extract prompt text from the JSON payload before pattern matching. | P1 |
| `error-capture.sh` | Remove `[error-capture]` bracket from log printf — inconsistent with all other scripts. Add `session=` field to log line. | P1 |
| All scripts | Output JSONL to `~/.mag/auto-capture.jsonl` instead of plain text to `auto-capture.log`. Keep a `START_NS=$(date +%s%N)` at script top and compute `duration_ms` before writing. | P0 |
| `plugin/hooks/hooks.json` | Add `SubagentStop` handler pointing to a new `subagent-end.sh` script (or reuse `session-end.sh` with `hook_event_name` discrimination). Emit `event: "hook.subagent_end"` with importance 0.3 (per ratified decision D4). | P1 |
| All scripts | Wrap the `mag` CLI call in a subshell; capture exit code and stderr; populate `hook.status` and `hook.error` in JSONL output. | P2 |
| `src/uninstall.rs` | Add `auto-capture.jsonl` (and legacy `auto-capture.log`) to the file removal list in the `~/.mag` cleanup path. | P1 |

---

## 6. Work Streams

1. **JSONL schema finalization** — this doc + Palantir debate on D1-D7. Output: ratified schema, decisions locked.
2. **Dev plugin setup** — create `plugin/dev/` with isolated `~/.dev-mag/` environment. Unblocks safe iteration.
3. **Hook script updates** — stdin parsing fixes (P0), JSONL output (P0), then P1/P2 fixes. One PR per logical group.
4. **Test harness** — `tests/hooks/` scripts using `claude -p --model haiku --include-hook-events`. Before/after `mag list --json` diffs as assertions.
5. **GitHub issues** — file individual issues for each P0/P1 item so work is tracked and linkable from PRs.
6. **Palantir debate** — present D1-D7 for ratification. D2 (reranker default) and D3 (SubagentStop) are the high-stakes ones.
7. **Dashboard / UI** — future, post-JSONL stabilisation. Blocked on stable schema.

---

## 7. Dev Plugin Architecture

The dev plugin is **development infrastructure for the hook system** — not a manual testing target. It provides an isolated sandbox, JSONL observability, and automated regression checks that make the coding improvement / evaluation lifecycle robust. When iterating on hook behavior (by humans or AI agents), changes are proven in the dev environment before graduating to production via a simple MAG_DATA_ROOT path change.

**Directory layout:**

```
~/.dev-mag/               # parallel to ~/.mag/
  memory.db               # dev DB (fresh or cloned from live)
  auto-capture.jsonl      # dev log — separate from production
  state/                  # pre-compact snapshots (dev)
```

**Plugin variant:**

```
plugin/dev/
  .mcp.json               # points to `mag serve --data-dir ~/.dev-mag/`
  hooks/
    hooks.json            # identical to plugin/hooks/hooks.json
    scripts/              # symlinks or copies of plugin/scripts/
                          # overriding LOG path to ~/.dev-mag/auto-capture.jsonl
```

**Setup options:**

```bash
# Fresh DB (clean slate)
mag setup --data-dir ~/.dev-mag/

# Clone from live (realistic test data, non-destructive)
cp ~/.mag/memory.db ~/.dev-mag/memory.db
```

**Test harness scripts** live in `tests/hooks/`. Each script:
1. Captures `mag list --json` baseline.
2. Runs `claude -p "..." --model haiku --include-hook-events --output-format stream-json`.
3. Captures `mag list --json` after.
4. Diffs the two snapshots and asserts expected memory was stored.
5. Validates `~/.dev-mag/auto-capture.jsonl` contains a well-formed JSONL entry with correct `event`, `session_id`, and `project` fields.

---

## 8. Future: Dashboard and Dream State

These are post-JSONL features. They require a stable, populated log before implementation begins.

**Web dashboard** — visualise hook activity, memory operation counts, session timelines, and hook latency (from `hook.duration_ms`). JSONL feeds directly into a simple SQLite loader or streaming parser.

**Batch graph-building** — correlate sessions to memories to graph connections. JSONL provides the event stream. Pattern: ingest log on `mag serve` startup, build adjacency edges, expose via MCP.

**Pattern detection** — same error (`hook.error_capture` + matching `context.error_line`) across N sessions triggers automatic importance promotion on the stored memory.

**Cross-agent attribution** — `agent.tool` and `agent.id` fields enable queries like "which subagent stored this memory?" and "how many memories did Codex vs Claude Code produce this week?"

Get the data flowing first. The schema is designed to make all of the above straightforward — no retroactive log munging required.
