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
- `mag.runtime.substrate` — unselected candidate composition path, outside this directory.

## Current constraint

`dec.select-local-runtime-composition-root` selects one entrypoint-owned local
runtime over the current SQLite-backed implementation. Do not add product
behaviour to the legacy `Pipeline` or feature-gated substrate. Fold a narrow
boundary into the live path only through a tracked migration slice with
public-surface parity coverage.

Before changing or removing a compatibility path, use:

```bash
cairn rationale mag.runtime.memory
cairn brief todo.introduce-local-memory-runtime-facade
cairn brief todo.retire-legacy-and-substrate-orchestration
```

The legacy `Pipeline`'s observable `processed: ` content remains protected until
a separate versioned migration decision changes it.

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
