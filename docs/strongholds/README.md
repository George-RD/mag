# Strongholds archive

The files in this directory are historical reconnaissance, adversarial review,
campaign, and implementation records. They preserve useful evidence about the
repository at the date recorded in each document, but they are not current
architecture or work-status authority.

Use these sources for historical context only. Before acting on a claim, verify
it against the current code and query Cairn for the live boundary:

```bash
cairn context
cairn bundle <node>
cairn rationale <node>
cairn status
cairn next
```

Current authority is:

1. the user's request;
2. accepted Cairn decisions and contracts for the affected node;
3. `docs/specs/local-first-roadmap.md` for mission, dependency order, and gates;
4. live Cairn todos for status and next work.

In particular, the historical substrate campaign and trait-surface plans do not
authorize a second production path. `dec.select-local-runtime-composition-root`
selects one entrypoint-owned local runtime over the current SQLite-backed
implementation and retains substrate only during migration and retirement.

Historical findings may be wrong after later changes. Do not copy their line
counts, feature status, tool counts, or completion claims into current planning
without re-verifying them.
