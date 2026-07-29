# `src/` scoped agent guide

The root `AGENTS.md` and accepted Cairn decisions remain authoritative. This file
contains only runtime-specific reminders; use Cairn for the live architecture.

## Orient to the owning node

```bash
cairn locate <symbol>
cairn context --scope <node>
cairn bundle <node>
```

Common nodes:

| Area | Cairn node |
|---|---|
| CLI, crate surface, process assembly, doctor dispatch | `mag.runtime.entrypoints` |
| Setup, configuration, paths, uninstall | `mag.runtime.setup` |
| MCP protocol and tools | `mag.runtime.mcp` |
| Memory domain/models/retrieval/storage | `mag.runtime.memory` |
| Feature-gated candidate orchestration; tests/benchmarks only | `mag.runtime.substrate` |
| Feature-gated HTTP support primitives; no assembled server | `mag.runtime.daemon` |

Do not use this file as a source-tree map. The blueprint and node contracts are
the maintained index.

## Runtime invariants

- Keep Clap declarations and runtime dispatch aligned.
- MCP stdio reserves stdout for JSON-RPC; send logs to stderr.
- Public CLI or MCP changes require compatibility consideration and tests.
- Synchronous SQLite work called from async code must use `spawn_blocking`.
- `SqliteStorage` is the audited current production semantic centre;
  `memory_core::Pipeline` is a compatibility-sensitive CLI adapter.
- Do not duplicate new production behaviour across the legacy/direct SQLite path
  and `substrate` while the composition decision is unresolved.
- Do not advertise a setup transport unless the current binary serves it
  end-to-end.
- New files and cross-node calls must be represented in Cairn.

Finish with focused tests, then `cairn scan` and `cairn hook all`.
