# `src/memory_core/` scoped agent guide

Use the root `AGENTS.md` first. This file narrows work inside the memory system;
Cairn remains the live architecture and decision index.

## Select the smallest node

```bash
cairn context --scope mag.runtime.memory
cairn bundle mag.runtime.memory.<domain|models|retrieval>
cairn bundle mag.runtime.memory.storage.<api|sqlite|memory>
cairn rationale mag.runtime.memory
```

Relevant nodes:

- `mag.runtime.memory.domain` — types, traits, and legacy `Pipeline`;
- `mag.runtime.memory.models` — embeddings, optional generation, model adapters;
- `mag.runtime.memory.retrieval` — candidate retrieval, reranking, scoring, abstention;
- `mag.runtime.memory.storage.sqlite` — production storage and query pipeline;
- `mag.runtime.memory.storage.memory` — reference backend and parity target;
- `mag.runtime.substrate` — candidate composition path, outside this directory.

## Current constraint

The production composition root is under active review. Do not add equivalent
behaviour to both the legacy `Pipeline` path and `substrate`. Before changing
or deleting either, use:

```bash
cairn brief todo.audit-current-architecture-and-dead-code
```

and trace current callers, feature flags, tests, benchmarks, and fallbacks.

## Memory-system invariants

- Preserve raw memories; generated facts, relations, clusters, and summaries are
  derived records with source provenance.
- Keep schema changes additive unless an accepted migration decision permits more.
- Preserve parity between production SQLite and the reference backend where the
  shared contract requires it.
- Retrieval, scoring, reranking, or query-pipeline changes require
  `./scripts/bench.sh --gate`.
- Use hermetic storage and model paths in tests; never touch the user's MAG data.
- Run synchronous SQLite work from async paths through `spawn_blocking`.

After focused tests, run `cairn scan` and `cairn hook all`.
