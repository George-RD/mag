# Known Gaps — Hook Integration Tests

## Dual-target mode (`HOOKS_TARGET`)

The test harness supports two plugin targets via the `HOOKS_TARGET` environment
variable:

| Value | Scripts dir | `MAG_DATA_ROOT` | Default? |
|---|---|---|---|
| `production` | `plugin/scripts/` | `~/.mag` | yes |
| `dev` | `plugin/dev/scripts/` | `~/.dev-mag` | no |

### Running in dev mode

```sh
HOOKS_TARGET=dev sh tests/hooks/run_all.sh
```

All six tests (t01–t06) work in both modes. `run_all.sh` prints the active
target at the start of output so it's visible in CI logs.

### SubagentStop is dev-only

`subagent-end.sh` is only present in `plugin/dev/scripts/`. In **production**
mode, `plugin-install.sh` detects the missing script and omits the
`SubagentStop` hook block from the generated `settings.json`, so t06 will
always skip rather than error in production mode.

## Hooks not testable via `claude -p`

### PreCompact / PostCompact

The `PreCompact` and `PostCompact` hooks only fire during an active conversation
when the context window is approaching its limit. `claude -p` (non-interactive,
single-turn mode) never triggers compaction, so these hooks cannot be exercised
by this test harness.

### Multi-turn session_id continuity

Claude Code maintains a stable `CLAUDE_SESSION_ID` across turns within a single
conversation. Testing that the session ID is the same value from `SessionStart`
through `Stop` requires `--resume <session-id>`, which is not available in the
`claude -p` invocation model used here. Each `run_claude` call produces an
independent session, so cross-turn ID continuity is not verified.

### Subagent spawning (t06)

Whether a model spawns a subagent in response to "Use a subagent to calculate
2+2" depends on the model's behavior and the `--max-turns` budget. Smaller
models (e.g. Haiku) may answer inline without spawning an agent. `t06` handles
this by skipping when `hook.subagent_end` is not fired, rather than failing.

## jq dependency

All hook scripts require `jq` for full JSONL telemetry output. Without jq, hooks
fall back to printf-based output that omits several fields: `agent`, context
sub-fields (e.g. `commit_message`, `vcs_tool`, `error_line`), and `memory` blocks.
The fallback output is valid JSON but structurally incomplete. Install jq for
production use: https://jqlang.github.io/jq/download/

## Other limitations

- **JSONL log location**: The current production hooks write plain-text lines to
  `$HOME/.mag/auto-capture.log`. The test harness targets the dev JSONL format
  at `$HOME/.dev-mag/auto-capture.jsonl`, which requires the dev variants of
  the hook scripts that emit structured JSON per event.

- **Cost**: Each `run_claude` invocation bills against the API. The harness caps
  spend at `--max-budget-usd 0.05` per call, but running all six tests will
  consume real tokens. Use `CLAUDE_MODEL=haiku` (the default) to minimise cost.

- **Idempotency**: Tests are designed to be idempotent — they do not leave
  persistent state in the repo or file system beyond what `teardown_test`
  removes. However, memories written to `$MAG_DATA_ROOT` by the session-end
  hook accumulate across runs.
