# MAG agent guide

MAG is a local-first Rust memory server for AI agents. This file is a routing
layer: load only the context required for the current task.

## Authority and context discipline

- The user's request is primary.
- Accepted Cairn decisions and contracts bind the architecture nodes they name.
- This file and any nearer scoped `AGENTS.md` define working conventions.
- Surface conflicts rather than silently choosing one source.
- Do not preload `map.md`, `map.json`, full roadmaps, `docs/strongholds/`, or
  `conductor/`. Query the smallest connected Cairn slice instead.

## Start every task with the smallest useful query

| Starting point | First query |
|---|---|
| A planned item or named todo | `cairn brief todo.<slug>` |
| A named symbol | `cairn locate <symbol>`, then `cairn bundle <node>` |
| A known architecture area | `cairn context --scope <node>` |
| A broad or unfamiliar request | `cairn context`, choose the owning node, then `cairn bundle <node>` |
| Current project priorities | `cairn status` and `cairn next` |
| Why a boundary exists | `cairn rationale <node>` |
| Connected work or evidence | `cairn neighbourhood <node>` with only the needed `--include-*` flags |

Identify the owning node, contract, binding decisions, and relevant todo before
reading a large source surface. Use `--json` when structured output is easier to
consume.

## Task routing

The Cairn router is `.claude/skills/cairn-dev/SKILL.md`. Read only the reference
matching the task:

| Task | Reference |
|---|---|
| Bug investigation | `.claude/skills/cairn-dev/references/task-bug-investigation.md` |
| Feature implementation | `.claude/skills/cairn-dev/references/task-feature-implementation.md` |
| Behaviour-preserving refactor | `.claude/skills/cairn-dev/references/task-refactoring.md` |
| Architecture or dead-code investigation | `.claude/skills/cairn-dev/references/task-architecture-discovery.md` |
| Cairn artefacts or blueprint edits | `.claude/skills/cairn-dev/references/artefact-schemas.md` or `blueprint-syntax.md` |
| Clean VM, model, MCP, or retrieval evaluation work | `skills/mag-development/SKILL.md` |

Clients without skill support may read the referenced file directly. Do not load
all references.

## Current architecture constraint

`dec.select-local-runtime-composition-root` selects one entrypoint-owned,
transport-independent local runtime over the current SQLite-backed implementation.
Until its migration slices are complete:

- route new production behaviour through the selected runtime boundary;
- treat direct SQLite callers and `memory_core::Pipeline` as tracked compatibility
  paths, not new extension points;
- do not add independent behaviour to the unselected feature-gated substrate;
- use `cairn brief todo.introduce-local-memory-runtime-facade` for the next
  implementation unit and `cairn rationale mag.runtime.entrypoints` for the
  binding decision.

## Repository invariants

- Core memory quality must work locally without cloud credentials. Hosted mode is
  an optional deployment and synchronization adapter over the same semantics.
- MCP stdio reserves stdout for protocol messages; runtime logs go to stderr.
- Run synchronous SQLite work from async paths through `spawn_blocking`.
- Schema evolution is additive unless an accepted migration decision says otherwise.
- Derived facts, relationships, clusters, and summaries retain source provenance
  and never overwrite raw memories.
- Changes to retrieval, scoring, reranking, or the SQLite query pipeline are
  benchmark-gated.

## Keep Cairn current

Update the graph as part of the same change when you:

- add or move source files across node ownership;
- introduce a cross-node dependency;
- change a public interface or module obligation;
- settle an architectural decision;
- start, block, or complete a tracked todo.

Todo status lives in `meta/todos/`, not roadmap checkboxes:

```bash
cairn todo set <slug> <open|in_progress|done|blocked>
```

Before handing work back:

```bash
cairn scan
cairn hook all
```

Record Cairn friction rather than working around it silently:

```bash
cairn feedback "what you expected, what happened instead"
```

## Engineering gates

Run the smallest focused tests during development. Before merge:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Additionally:

- retrieval/scoring/storage-pipeline changes: `./scripts/bench.sh --gate`;
- CLI, MCP, installation, or model-startup changes: `bash scripts/smoke-test.sh`;
- clean-environment or local-model work: follow `skills/mag-development/SKILL.md`.

<!-- cairn:agent-guide-begin -->
## Cairn orientation

This project uses cairn to keep its architecture map in sync with code. Read
`.cairn/AGENTS.md` for full orientation, then follow
`.claude/skills/cairn-dev/SKILL.md` for the development loop.
<!-- cairn:agent-guide-end -->
