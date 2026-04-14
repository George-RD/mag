# MAG Dev Plugin Testing Skill — Specification

**File:** `docs/specs/testing-skill.md`
**Status:** Draft v1.0 — 2026-04-14
**Scope:** `plugin/dev/skills/mag-test.md` and all supporting files under `plugin/dev/`

---

## 1. Overview

### What

A Claude Code skill (`plugin/dev/skills/mag-test.md`) that orchestrates five test modes against the MAG dev plugin: quick smoke checks, the full TAP hook suite, scenario-based recall quality tests, a longitudinal regression suite, and an interactive tmux environment for exploratory testing.

### Why

The TAP suite (`tests/hooks/`) validates hook wiring — that events fire and JSONL fields are correct. It cannot validate recall quality: whether MAG surfaces the right memories in the right context. Only real Claude Code sessions with controlled memory databases can answer that. The testing skill closes this gap by running actual `claude` processes against known DB states and asserting on what Claude says back.

The secondary benefit is accumulation. Each scenario run adds a timestamped data point. Over weeks of development this becomes a real-world regression suite that no synthetic dataset can replicate. Continuous scoring metrics (true benchmark mode with quantitative recall/precision tracking) are planned for v2.

### Scope boundary

This skill does not replace the TAP suite. It augments it. The TAP suite remains the authoritative gate for hook correctness. The testing skill is for recall quality, regression detection, and interactive exploration.

---

## 2. Architecture

### Components

```
plugin/dev/
  skills/
    mag-test.md               <- the skill (this spec)
    mag-dev-status.md         <- existing status skill (unchanged)
  fixtures/
    README.md                 <- fixture inventory and privacy policy
    seeded/
      basic-recall.json       <- 5 generic memories, tests baseline recall
      project-context.json    <- project-specific memories, tests project isolation
      multi-session.json      <- memories from 3 synthetic sessions
  scenarios/
    s01-basic-recall.yaml
    s02-project-isolation.yaml
    s03-error-pattern.yaml
    s04-commit-context.yaml
  logs/
    test-runs.jsonl           <- one line per run: timestamp, mode, pass/fail, scores
    discoveries.md            <- human-written learning log
    decisions.md              <- decisions made based on test outcomes
  run-scenario.sh             <- executes one scenario, writes result line
  run-benchmark.sh            <- executes all scenarios, aggregates scores
tests/hooks/
  helpers/common.sh           <- existing (unchanged)
  helpers/plugin-install.sh   <- existing (unchanged)
  t01-t06.sh                  <- existing (unchanged)
```

### Data flow for scenario mode

```
Scenario YAML
  -> run-scenario.sh reads prompt, fixture_name, assertions
  -> loads fixture into ~/.dev-mag/memory.db via mag import
  -> invokes claude -p <prompt> --model <model>
  -> captures stdout (Claude's response) + new JSONL lines
  -> evaluates assertions (substring / JSONL field / memory count)
  -> appends one result line to plugin/dev/logs/test-runs.jsonl
  -> prints PASS/FAIL with details
```

### Live plugin isolation

**Decision:** Use `CLAUDE_CODE_HOME` env var for full isolation. `disabledPlugins` is NOT a real Claude Code setting. Instead, `CLAUDE_CODE_HOME` points Claude Code at a completely separate environment with no plugins loaded except what is explicitly configured there.

The test-env.sh invocation becomes:

```sh
CLAUDE_CODE_HOME=/tmp/mag-test-claude claude -p "..." --model haiku
```

`test-env.sh` creates `/tmp/mag-test-claude/settings.json` with only the dev plugin path:

```json
{
  "plugins": [{"path": "<plugin/dev absolute path>"}]
}
```

This guarantees the live (marketplace-installed) MAG plugin does not fire — the isolated `CLAUDE_CODE_HOME` has no knowledge of it.

**Alternative approach:** `claude plugin disable mag --scope project` in the test repo. This modifies the project-level settings to disable the production plugin. Less clean than `CLAUDE_CODE_HOME` because it mutates the test repo's settings and does not isolate other user-level configuration.

The TAP suite uses `plugin-install.sh` which writes raw hook paths into `settings.json` and never activates the named plugin at all, so the TAP suite is already isolated by construction.

---

## 3. Test Modes

### 3.1 smoke

**Purpose:** Confirm the dev plugin is wired up and session-start fires. Takes ~5 seconds. Zero configuration required.

**What it does:**
1. Pre-flight: verifies `/tmp/mag-test-repo` exists with `settings.local.json` (exits with instructions to run `test-env.sh` if missing).
2. Verifies `~/.dev-mag/bin/mag` exists.
3. Runs `CLAUDE_CODE_HOME=/tmp/mag-test-claude claude -p "Echo HELLO" --model haiku --max-turns 1 --dangerously-skip-permissions` from the test repo directory.
4. Checks `~/.dev-mag/auto-capture.jsonl` for a `hook.session_start` event in the last 20 lines. The `tail -20` approach is intentionally loose for smoke mode -- it confirms hook wiring, not event attribution.
5. Prints PASS with event timestamp, or FAIL with the last 5 JSONL lines.

**Model:** Haiku (cheapest; hook validation only).

**Does not require:** tmux, fixtures, scenarios.

### 3.2 hooks

**Purpose:** Run the full TAP suite against the dev plugin. This is a wrapper around the existing `plugin/dev/run-tests.sh`.

**What it does:**
1. Calls `plugin/dev/run-tests.sh` with `--model haiku`.
2. Captures the TAP output and exit code.
3. Reports pass/fail counts to the user.

**Model:** Haiku.

**Accepts optional flags:**
- `--filter <glob>` — passed through to `run-tests.sh`.
- `--model <name>` — overrides Haiku for specific tests.

### 3.3 scenario

**Purpose:** Run a specific prompt against a controlled DB, assert recall quality.

**Arguments:** `--scenario <name>` (e.g. `s01-basic-recall`). Required.

**What it does:**
1. Reads `plugin/dev/scenarios/<name>.yaml`.
2. Loads the named fixture into `~/.dev-mag/memory.db` (via `run-scenario.sh`).
3. Runs `claude -p <prompt>` from the test repo.
4. Evaluates assertions.
5. Writes one line to `plugin/dev/logs/test-runs.jsonl`.
6. Prints PASS/FAIL.

**Model:** Configurable per scenario (default: Haiku). Use Sonnet only when the scenario explicitly requires semantic nuance (see Section 7).

### 3.4 benchmark (regression suite)

**Purpose:** Run all scenarios, score each, produce a summary table, track regressions over time. In v1 this is a pass/fail regression suite, not a continuous scoring benchmark. Quantitative recall/precision metrics are planned for v2.

**What it does:**
1. Iterates over all `plugin/dev/scenarios/*.yaml` in filename order.
2. Runs each via `run-scenario.sh`.
3. Aggregates pass/fail and per-scenario scores (binary 0.0/1.0 in v1).
4. Prints a summary table.
5. Appends a single summary line (with `"line_type": "benchmark_summary"`) to `plugin/dev/logs/test-runs.jsonl`.

**Model:** Per-scenario from YAML; defaults to Haiku.

**Run cadence:** Manual. Run before and after significant algorithm changes. Do not run in CI (cost, latency).

### 3.5 interactive

**Purpose:** Set up a tmux environment for manual exploratory testing. Leaves the user in a live Claude session with dev plugin active and telemetry visible in a split pane.

**What it does:**
1. Runs `plugin/dev/test-env.sh --no-build` to ensure the test repo exists.
2. Creates (or re-uses) the `mag-test` tmux session with the two-pane layout.
3. Prints the attach command.

**Does not start Claude automatically** — the user attaches and types `claude` themselves. This avoids the tool controlling an interactive session.

---

## 4. DB Fixture System

### Storage location

**Decision: in-repo at `plugin/dev/fixtures/seeded/`.**

Rationale: Version-controlled fixtures enable reproducible scenario runs across machines and contributors. The fixtures are synthetic (not cloned from a real user's DB), so there is no privacy concern. The `plugin/dev/fixtures/README.md` enforces this: it explicitly states that fixtures must not contain real user data.

The `--clone` workflow (copying `~/.mag/memory.db` to `~/.dev-mag/memory.db`) remains available for personal regression testing but produces no committed artifacts.

### Fixture format

Fixtures are JSON files produced by `mag export` and consumed by `mag import`. The schema is whatever `export_all()` emits from `src/memory_core/storage/sqlite/mod.rs`. No wrapper format is added.

### Fixture creation workflow

```sh
# Seed memories manually
# IMPORTANT: --project must match basename of the test repo CWD (/tmp/mag-test-repo).
# MAG uses exact-match project filtering (WHERE project = ?), not fuzzy/boost.
# Memories stored with --project test will NOT surface when querying with --project mag-test-repo.
MAG_DATA_ROOT=~/.dev-mag mag process "George prefers concise answers" \
  --event-type preference --project mag-test-repo --importance 0.8

# Export to a named fixture
MAG_DATA_ROOT=~/.dev-mag mag export > plugin/dev/fixtures/seeded/basic-recall.json
```

### Fixture loading in run-scenario.sh

```sh
# Reset dev DB to a known state
rm -f ~/.dev-mag/memory.db
MAG_DATA_ROOT=~/.dev-mag mag import plugin/dev/fixtures/seeded/<fixture>.json
```

The DB is wiped before import to guarantee deterministic state. The `~/.dev-mag/auto-capture.jsonl` is NOT wiped between scenario runs — the JSONL mark pattern from `common.sh` (snapshot line count before, tail from mark after) handles isolation.

### Fixture inventory

| File | Contents | Used by |
|---|---|---|
| `basic-recall.json` | 5 generic preference/fact memories | s01 |
| `project-context.json` | 8 memories tagged to project "alpha" | s02 |
| `multi-session.json` | 15 memories across 3 session-ids | s03, s04 |

---

## 5. tmux Automation

### Session layout

```
Session: mag-test
Window 0: mag-test
  Pane 0 (left, 75%):  cd /tmp/mag-test-repo && $SHELL
  Pane 1 (right, 25%): tail -f ~/.dev-mag/auto-capture.jsonl | jq -r '[.ts,.event,.hook.status] | @tsv'
```

### Creation command (illustrative -- implementation calls `test-env.sh`)

```sh
tmux new-session -d -s mag-test -x 220 -y 50 \; \
  send-keys "cd /tmp/mag-test-repo" Enter \; \
  split-window -h -p 25 \; \
  send-keys "tail -f $HOME/.dev-mag/auto-capture.jsonl | jq -r '[.ts,.event,.hook.status] | @tsv'" Enter \; \
  select-pane -t 0
```

This is illustrative only. The skill invokes `test-env.sh --no-build` which handles session creation internally.

### Sandbox note

tmux commands require access to the Unix socket at `/private/tmp/tmux-$(id -u)/default`. Claude Code's sandbox blocks this by default. `run-scenario.sh` and `test-env.sh` must either:
- Run outside the Claude Code sandbox (with `dangerouslyDisableSandbox: true`), or
- The user must add the tmux socket path (`/private/tmp/tmux-*/`) to the sandbox filesystem allowlist.

### Automated session control (scenario/benchmark modes)

Scenario and benchmark modes do NOT use tmux for the `claude -p` invocations. They run `claude -p` as a subprocess directly (same as the TAP suite). tmux is only for `interactive` mode.

For future multi-turn testing (compaction, session continuity), the pattern is:

```sh
# Send a prompt to the Claude session running in pane 0
tmux send-keys -t mag-test:0.0 "your prompt here" Enter

# Wait for prompt to return (naive: sleep; robust: poll for the shell prompt)
sleep 5

# Capture pane output
tmux capture-pane -t mag-test:0.0 -p -S -50
```

This is documented in KNOWN_GAPS.md territory and is not implemented in the initial skill. The `interactive` mode leaves it to the user. A `multi-turn` scenario type is reserved for a future iteration.

### cmux note

`cmux send`/`read-screen` does not work cross-workspace. The skill uses raw `tmux` commands throughout, even when `cmux` is available on PATH. `test-env.sh` already handles the cmux/tmux fallback for session creation — the skill re-uses that script rather than duplicating the logic.

---

## 6. Scenario Format

Scenarios are YAML files. YAML was chosen over TOML for readability (multi-line strings are natural) and over shell scripts for parseability (no eval surface).

### Schema

```yaml
# plugin/dev/scenarios/s01-basic-recall.yaml
id: s01-basic-recall
description: "Verify that a simple preference memory surfaces in a recall query"
fixture: basic-recall          # filename without extension under fixtures/seeded/
model: haiku                   # haiku | sonnet | opus (default: haiku)
prompt: |
  What do you know about my preferences?
  List any facts or preferences you have stored about me.
assertions:
  - type: response_contains
    value: "concise"           # Claude's response must contain this substring
  - type: event_fired
    event: hook.session_start  # JSONL event must appear since test start
  # memory_recalled is not yet supported — see Section 7 and Q7.
  # The JSONL `memory` field is always null in the current session-start.sh.
  # This assertion type will be added when session-start.sh is patched to
  # capture `mag welcome` output into the JSONL memory field.
timeout_seconds: 30
notes: "Baseline recall test. If this fails, recall pipeline is broken."
```

### Assertion types

| Type | Parameters | Evaluation method |
|---|---|---|
| `response_contains` | `value: string` | Case-insensitive substring match in `claude -p` stdout |
| `response_not_contains` | `value: string` | Inverse substring match |
| `event_fired` | `event: string` | JSONL new lines contain at least one object with `.event == value` |
| `jsonl_field` | `event`, `path`, `expected` | `get_event \| jq -r path == expected` |
| `memory_stored` | `min_count: int` | Memory count delta >= min_count (uses the same pattern as `assert_memory_stored` in common.sh) |

**Note:** `memory_recalled` is **not available in v1**. The `memory` field in the `hook.session_start` JSONL event is hardcoded to `null` in `session-start.sh`. The `mag welcome` output goes to stdout as `additionalContext` for Claude but is never captured in JSONL. This assertion type will be added after `session-start.sh` is patched to capture `mag welcome` output into the JSONL `memory` field (see Phase 0 in Section 10 and Q7).

### Parsing

`run-scenario.sh` parses YAML with shell tools. The schema is intentionally flat to keep parsing tractable without a real YAML library.

**Approach — shell-native (no external dependency):**
- Simple key-value fields: `grep '^key:' file | sed 's/^key: *//'`
- Multi-line `prompt: |` block scalar: `awk` to read from `^prompt: \|` until the next non-indented key
- Assertion blocks: iterate by scanning for `- type:` delimiters, extracting subsequent indented key-value pairs

**Alternative — python3 (simpler, recommended if complexity grows):**
```sh
eval "$(python3 -c "
import yaml, json, sys
d = yaml.safe_load(open(sys.argv[1]))
print(f'SCENARIO_ID={d[\"id\"]}')
print(f'FIXTURE={d[\"fixture\"]}')
print(f'MODEL={d.get(\"model\",\"haiku\")}')
print(f'PROMPT={json.dumps(d[\"prompt\"])}')
print(f'ASSERTIONS={json.dumps(d[\"assertions\"])}')
" "$scenario_file")"
```
Python 3 with PyYAML is available on macOS and Linux. This avoids the awk complexity for multi-line block scalars entirely. The tradeoff is a runtime dependency on `python3` and `PyYAML` (present in most environments but not guaranteed).

---

## 7. Assertion System

### response_contains (and response_not_contains)

`claude -p` stdout is captured to a temp file. The assertion does a case-insensitive grep. This is intentionally loose — the goal is "did the memory surface in Claude's answer" not "did Claude say the exact string." If a scenario requires strict matching, use `response_contains` with a distinctive phrase that only appears in the seeded memory.

### event_fired

Uses the JSONL mark pattern from `common.sh`: snapshot `wc -l $JSONL_LOG` before running `claude -p`, then `tail -n +$(mark+1)` after. Filter with `jq 'select(.event == "...")'`. This is identical to `assert_event_fired` in the TAP suite.

### memory_recalled (NOT AVAILABLE IN v1)

The `memory` field in the `hook.session_start` JSONL event is **always `null`** — it is hardcoded in `session-start.sh`. The `mag welcome` output goes to stdout (as `additionalContext` for Claude) but is never captured in JSONL. Therefore `memory_recalled` assertions cannot be evaluated against the JSONL log.

**Fix path:** Patch `session-start.sh` to capture `mag welcome` stdout into a variable and write it into the JSONL `memory` field. This is tracked as a Phase 0 prerequisite in Section 10. Once patched, `memory_recalled` becomes: `jq '.memory != null'` on the session-start event, with `min_count` evaluated against a `memory.count` or array length field.

### Model selection per test mode

| Mode | Default model | Override |
|---|---|---|
| smoke | haiku | none |
| hooks | haiku | `--model` flag |
| scenario | per-scenario YAML field | `--model` flag overrides YAML |
| benchmark | per-scenario YAML field | `--model` flag overrides all scenarios |
| interactive | user chooses | n/a |

Use Sonnet only when a scenario's `description` indicates semantic nuance is required (e.g., paraphrase matching, multi-hop reasoning). The default of Haiku for all hook-wiring assertions is non-negotiable — Haiku costs ~20x less and hook correctness does not require intelligence.

---

## 8. Logging

All logs are in-repo under `plugin/dev/logs/`. This directory is committed but JSONL contents are gitignored via `plugin/dev/logs/.gitignore` containing `*.jsonl`. The markdown logs (`discoveries.md`, `decisions.md`) are committed.

### test-runs.jsonl

One JSONL line per scenario execution (and one summary line per benchmark run). Appended, never overwritten.

```json
{"ts":"2026-04-14T10:00:00Z","line_type":"scenario","run_id":"r1","mode":"scenario","scenario":"s01-basic-recall","fixture":"basic-recall","model":"haiku","result":"pass","score":1.0,"assertions":[{"type":"response_contains","value":"concise","result":"pass"}],"duration_ms":4200,"mag_version":"0.1.9-dev"}
```

For a regression suite run, an additional summary line:

```json
{"ts":"2026-04-14T10:01:00Z","line_type":"benchmark_summary","run_id":"r1","mode":"benchmark","scenarios_run":4,"passed":3,"failed":1,"score":0.75,"duration_ms":18000,"mag_version":"0.1.9-dev"}
```

The `line_type` field discriminates between per-scenario results and aggregate summaries. The `score` field is 0.0-1.0; in v1 it is binary (1.0 = all assertions pass, 0.0 = any failure). Continuous scoring (partial credit, recall precision metrics) is planned for v2.

`mag_version` is captured from `~/.dev-mag/bin/mag --version` at the start of each run.

### discoveries.md

Human-written. Format: dated entries, free prose. Example:

```
## 2026-04-14
Ran s02 for the first time. The project-context fixture revealed that recall
correctly scopes to the active project but leaks one memory tagged "global".
Filed as a candidate for investigation.
```

### decisions.md

Human-written. Format: dated decision records. Example:

```
## 2026-04-14 — Use Haiku for all hook assertions
Cost: ~$0.002 per TAP run. Sonnet would be ~$0.04.
Hook correctness is binary and does not require model intelligence.
Decision: Haiku for hooks, Sonnet optional per-scenario for recall quality.
```

### TAP logs (existing)

`run-tests.sh` already writes TAP logs to `~/.dev-mag/test-results-<timestamp>.tap`. These remain outside the repo (ephemeral). The skill does not change this.

---

## 9. Skill Interface

The skill lives at `/Users/george/repos/mag/plugin/dev/skills/mag-test.md`.

### Trigger phrases

- "run mag tests"
- "run smoke test"
- "test the dev plugin"
- "run scenario <name>"
- "run benchmark"
- "set up interactive testing"
- "show test results"

### Invocation patterns

The skill is a Claude Code skill — it provides instructions that guide the assistant to run specific shell commands. It does not execute autonomously; it tells Claude Code what to run and what to check.

### Argument parsing by Claude Code

The user's natural language maps to a mode. Claude Code parses intent:

| User says | Mode | Key parameters |
|---|---|---|
| "run smoke test" | smoke | none |
| "run hook tests" or "run tap suite" | hooks | optional `--filter`, `--model` |
| "run scenario s01" | scenario | `--scenario s01-basic-recall` |
| "run all scenarios" or "run benchmark" | benchmark | optional `--model` |
| "set up interactive" or "open test env" | interactive | none |
| "show test results" or "what did the last run show" | results | reads `logs/test-runs.jsonl` |

### Step-by-step instructions per mode (what the skill tells Claude Code to do)

**smoke:**
```sh
# 0. Pre-flight: ensure test environment exists
[ -d /tmp/mag-test-repo ] && [ -f /tmp/mag-test-repo/.claude/settings.local.json ] || {
  echo "MISSING: run plugin/dev/test-env.sh first"; exit 1
}

# 1. Pre-flight: ensure dev mag binary exists
ls ~/.dev-mag/bin/mag || echo "MISSING: run setup.sh --build"

# 2. Run claude from test repo (isolated via CLAUDE_CODE_HOME)
cd /tmp/mag-test-repo && \
  CLAUDE_CODE_HOME=/tmp/mag-test-claude \
  claude -p "Echo HELLO" \
    --model haiku \
    --max-turns 1 \
    --dangerously-skip-permissions \
    2>/dev/null

# 3. Check JSONL
# NOTE: The tail -20 approach is intentionally loose for smoke mode —
# it confirms hook wiring, not event attribution. A session-start event
# anywhere in the last 20 lines is sufficient to prove the dev plugin fired.
tail -20 ~/.dev-mag/auto-capture.jsonl | \
  jq -c 'select(.event == "hook.session_start")' | tail -1
```
Report: event present = PASS, absent = FAIL with last 5 raw lines.

**hooks:**
```sh
cd /path/to/repo && \
  sh plugin/dev/run-tests.sh [--filter <f>] [--model <m>]
```
Report: TAP summary line.

**scenario:**
```sh
sh plugin/dev/run-scenario.sh --scenario <name> [--model <m>]
```
Report: PASS/FAIL per assertion, duration, result line written to logs.

**benchmark:**
```sh
sh plugin/dev/run-benchmark.sh [--model <m>]
```
Report: summary table of all scenarios with scores.

**interactive:**
```sh
sh plugin/dev/test-env.sh --no-build
tmux attach -t mag-test
```

**results:**
```sh
tail -20 plugin/dev/logs/test-runs.jsonl | \
  jq -r '[.ts, .mode, .scenario // "all", .result // "\(.passed)/\(.scenarios_run)"] | @tsv'
```

---

## 10. Implementation Plan

### Phase 0 — Hook schema prerequisite

- [ ] Patch `plugin/dev/hooks/session-start.sh` to capture `mag welcome` stdout into a variable and write it into the JSONL `memory` field (currently hardcoded to `null`). This is a prerequisite for `memory_recalled` assertions. Without this patch, recall quality can only be asserted via `response_contains` (checking Claude's output for recalled content).

### Phase 1 — Foundation (prerequisite for all other phases)

- [ ] Create `plugin/dev/logs/` directory with `plugin/dev/logs/.gitignore` containing `*.jsonl`.
- [ ] Create `plugin/dev/logs/discoveries.md` and `plugin/dev/logs/decisions.md` (empty dated stubs).
- [ ] Create `plugin/dev/fixtures/seeded/` directory and `plugin/dev/fixtures/README.md`.
- [ ] Patch `plugin/dev/test-env.sh`: use `CLAUDE_CODE_HOME=/tmp/mag-test-claude` for plugin isolation. Create `/tmp/mag-test-claude/settings.json` with only the dev plugin path. Remove any `disabledPlugins` references (not a real Claude Code setting).
- [ ] Seed and export the three initial fixtures (`basic-recall.json`, `project-context.json`, `multi-session.json`) using the dev plugin manually, then commit. Use `--project mag-test-repo` (matching `basename` of `/tmp/mag-test-repo`) for all fixture memories.

### Phase 2 — run-scenario.sh

- [ ] Write `plugin/dev/run-scenario.sh`. Responsibilities:
  - Parse a scenario YAML file (grep/sed/awk for simple fields, awk for multi-line block scalars; or python3+PyYAML if available — see Section 6).
  - Accept `--scenario <name>` and optional `--model <name>`.
  - Reset `~/.dev-mag/memory.db` and load the named fixture via `mag import`.
  - Snapshot JSONL line count (JSONL mark pattern).
  - Run `claude -p <prompt>` from `/tmp/mag-test-repo`, capture stdout to tempfile.
  - Evaluate each assertion.
  - Append one result line to `plugin/dev/logs/test-runs.jsonl`.
  - Exit 0 on all-pass, exit 1 on any failure.
- [ ] Write the four initial scenario YAML files (`s01` through `s04`).
- [ ] Smoke-test `run-scenario.sh` against `s01-basic-recall` manually.

### Phase 3 — run-benchmark.sh

- [ ] Write `plugin/dev/run-benchmark.sh`. Responsibilities:
  - Iterate all `plugin/dev/scenarios/*.yaml`.
  - Call `run-scenario.sh` for each.
  - Aggregate pass/fail counts and per-scenario durations.
  - Print a summary table.
  - Append one benchmark-run summary line to `test-runs.jsonl`.
- [ ] Run the benchmark once to establish a baseline result line in the log.

### Phase 4 — Skill file

- [ ] Write `plugin/dev/skills/mag-test.md` with all five modes, step-by-step instructions, and the invocation table from Section 9.

### Phase 5 — Validation

- [ ] Run smoke mode: confirm PASS.
- [ ] Run hooks mode: confirm all 6 TAP tests pass (or skip with known reason).
- [ ] Run benchmark mode: confirm all 4 scenarios produce result lines in `test-runs.jsonl`.
- [ ] Run interactive mode: confirm tmux session created, telemetry tail visible.
- [ ] Verify `plugin/dev/logs/.gitignore` contains `*.jsonl` (created in Phase 1).

---

## 11. Open Questions

### Q1 — live plugin isolation (RESOLVED -- VERIFIED)

**Resolution:** `disabledPlugins` is NOT a real Claude Code setting. Verified approach: use `CLAUDE_CODE_HOME=/tmp/mag-test-claude` to create a fully isolated Claude Code environment. This env has its own `settings.json` with only the dev plugin path configured. The live (marketplace-installed) MAG plugin is invisible to the isolated environment.

Alternative verified approach: `claude plugin disable mag --scope project` in the test repo. Less clean because it mutates the test repo's settings.

The TAP suite is already isolated via `plugin-install.sh` and does not need this change.

### Q2 — Fixture privacy (RESOLVED)

**Resolution:** Only synthetic fixtures live in-repo. The `--clone` workflow produces no committed artifacts. `plugin/dev/fixtures/README.md` states this explicitly. A `.gitignore` entry in `plugin/dev/fixtures/` ignores `*.db` to prevent accidental DB commits.

### Q3 — Scenario format (RESOLVED)

**Resolution:** YAML with grep/awk parsing. No external dependency. Schema is intentionally flat. See Section 6.

### Q4 — Skill orchestration model (RESOLVED)

**Resolution:** Semi-automated. The skill provides step-by-step shell commands for Claude Code to execute. It does not drive tmux for `scenario`/`benchmark` modes — those run `claude -p` as a direct subprocess. tmux is only for `interactive` mode setup. This avoids cross-workspace cmux issues entirely.

### Q5 — Multi-turn testing (DEFERRED)

**Status:** Documented in KNOWN_GAPS.md territory. Not in scope for initial implementation. The `interactive` mode leaves multi-turn testing to the user. A `multi-turn` scenario type that uses `tmux send-keys` + `tmux capture-pane` is reserved for a future iteration after the basic scenario/benchmark infrastructure is validated.

**Trigger for deferral:** PreCompact/PostCompact hooks require a context-filling conversation. This is only testable interactively or with a very long synthetic prompt. Neither approach is clean enough to automate in the first iteration.

### Q6 — Cost control (RESOLVED)

**Resolution:** Haiku for all hook/wiring assertions (smoke, hooks, any scenario not explicitly requiring Sonnet). Sonnet is opt-in per-scenario via the `model:` YAML field. The `--model` CLI flag overrides everything. The skill notes in its documentation that benchmark runs with all-Sonnet cost approximately $0.05-0.10 per full run (4 scenarios x ~5 turns each).

### Q7 — Recall assertion for memory_recalled (VERIFIED BROKEN -- FIX PLANNED)

**Verified finding:** The `memory` field in `hook.session_start` JSONL events is **always `null`** — it is hardcoded in `session-start.sh`. The `mag welcome` output goes to stdout (as `additionalContext` for Claude) but is never captured in the JSONL event.

**Fix path:** Phase 0 in Section 10 adds a prerequisite: patch `session-start.sh` to capture `mag welcome` stdout into a variable and write it into the JSONL `memory` field. Once patched, the `memory_recalled` assertion type can be re-added to the assertion types table (Section 6) with evaluation via `jq '.memory != null'` and optional `min_count` against an array length or count field.

**Current workaround:** Use `response_contains` assertions to verify recall quality indirectly — if Claude's response includes content from the seeded memories, recall is working. This is less precise but sufficient for v1.

---

## Appendix A — File Paths Reference

| Path | Description |
|---|---|
| `plugin/dev/skills/mag-test.md` | The skill file (new) |
| `plugin/dev/run-scenario.sh` | Scenario executor (new) |
| `plugin/dev/run-benchmark.sh` | Benchmark runner (new) |
| `plugin/dev/fixtures/seeded/basic-recall.json` | Fixture (new) |
| `plugin/dev/fixtures/seeded/project-context.json` | Fixture (new) |
| `plugin/dev/fixtures/seeded/multi-session.json` | Fixture (new) |
| `plugin/dev/fixtures/README.md` | Fixture policy (new) |
| `plugin/dev/scenarios/s01-basic-recall.yaml` | Scenario (new) |
| `plugin/dev/scenarios/s02-project-isolation.yaml` | Scenario (new) |
| `plugin/dev/scenarios/s03-error-pattern.yaml` | Scenario (new) |
| `plugin/dev/scenarios/s04-commit-context.yaml` | Scenario (new) |
| `plugin/dev/logs/test-runs.jsonl` | Run log (new, gitignored) |
| `plugin/dev/logs/discoveries.md` | Learning log (new, committed) |
| `plugin/dev/logs/decisions.md` | Decision log (new, committed) |
| `plugin/dev/test-env.sh` | Patch: use `CLAUDE_CODE_HOME` isolation |
| `tests/hooks/KNOWN_GAPS.md` | Reference: known limitations |
| `tests/hooks/helpers/common.sh` | Reference: assertion helpers |

## Appendix B — Key Design Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| Fixture storage | In-repo `plugin/dev/fixtures/seeded/` (synthetic only) | Version-controlled, portable, no privacy risk |
| Scenario format | YAML, grep/sed/awk parsed (python3+PyYAML alternative) | Readable, flat schema; python3 fallback for multi-line block scalars |
| Orchestration model | Semi-automated (subprocess for `claude -p`, tmux only for `interactive`) | Avoids cross-workspace cmux issues, simpler than full tmux automation |
| Live plugin isolation | `CLAUDE_CODE_HOME` env var for full environment isolation | Creates separate Claude Code env; no `disabledPlugins` (not a real setting) |
| Model for hook assertions | Haiku | Binary correctness, 20x cheaper than Sonnet |
| Model for recall scenarios | Per-scenario YAML, default Haiku | Cost control with opt-in quality |
| Recall assertion method | Substring match + JSONL event field check | Deterministic, no LLM judge cost for initial pass; LLM judge deferred |
| Multi-turn testing | Deferred to future iteration | PreCompact untestable via `claude -p`; complexity not justified yet |
| Log format | JSONL for machine data, Markdown for human notes | Queryable history + narrative context in the same place |
