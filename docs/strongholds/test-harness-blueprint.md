# Test Harness Blueprint

**Status:** approved
**Last updated:** 2026-04-09

> Architecture for `tests/hooks/` — claude -p based end-to-end hook testing with JSONL validation.

## Structure

```
tests/hooks/
├── helpers/
│   ├── common.sh          # shared setup/teardown, assert functions, JSONL helpers
│   └── plugin-install.sh  # per-test plugin installation into tmpdir config
├── t01_session_start.sh   # SessionStart fires, session_id populated
├── t02_session_end.sh     # Stop fires, last_assistant_message captured
├── t03_commit_capture.sh  # jj/git commit, commit message stored
├── t04_error_capture.sh   # failing cargo build, error_pattern stored
├── t05_prompt_gate.sh     # "remember" keyword, hint emitted
├── t06_subagent_stop.sh   # spawn subagent, subagent_end event
├── KNOWN_GAPS.md          # PreCompact/PostCompact untestable
└── run_all.sh             # runner with TAP output
```

## Purpose

The dev plugin and test harness are **development infrastructure for the hook system** — not something the user manually tests. They provide:

- **Isolated sandbox** — experiment with hook changes without poisoning production memories
- **JSONL observability** — structured, queryable output from every hook
- **Automated regression checks** — verify hooks work after every change
- **Graduation path** — once hooks are proven in dev, the diff to production is a MAG_DATA_ROOT change

This is the staging environment for hook development — it makes the coding improvement / evaluation REPL lifecycle robust.

## Key design decisions

- Each test uses `--config-dir $TMPDIR` for isolated Claude Code config
- JSONL_MARK (line count before test) ensures idempotent assertions
- `mag list --json` before/after diff for memory verification
- Sequential execution only (no parallel — JSONL_MARK race)
- Exit 77 = skip (automake convention)
- `--max-budget-usd 0.05` per test, ~$0.06 typical for full suite
- Tests use the locally installed `claude` CLI (no separate API key config needed)
- Label-gated CI: only runs on PRs with `test-hooks` label

## Known gaps

- PreCompact/PostCompact: cannot trigger via claude -p
- Multi-turn session continuity: needs --resume chain testing
- Subagent spawning: depends on model deciding to use Task tool

## Critical assumption

`claude -p --config-dir $DIR` must load hooks from that config dir. Validate in t01 first. Fallback: write hooks to ~/.claude/settings.json temporarily.
