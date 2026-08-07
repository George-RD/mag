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

`dec.select-local-runtime-composition-root` selects a CLI-first,
entrypoint-owned, transport-independent local runtime over the current
SQLite-backed implementation.

- The CLI is MAG's canonical operating surface for people, skills, automation,
  and local tools. Prefer CLI commands unless an integration specifically
  requires another transport.
- MCP is an optional stdio transport adapter over the same runtime capabilities.
  It must not construct its own runtime, own storage, or grow independent memory
  semantics.
- Production core and runtime modules compile through the library crate once. Do
  not reintroduce binary-private copies of `memory_core` or
  `local_memory_runtime`.
- Route new production behaviour through `LocalMemoryRuntime`. Treat direct
  SQLite callers and `memory_core::Pipeline` as tracked compatibility paths, not
  new extension points.
- Do not add independent behaviour to the unselected feature-gated substrate.
- Do not hard-code a completed todo as the next task. Reconcile open PRs and
  branches, then use `cairn status`, `cairn next`, and the binding rationale to
  select current work.

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

- determine whether a local change is benchmark-governed with
  `python3 scripts/retrieval_benchmark_gate.py --base main --head HEAD --explain`;
  when it prints `true`, run `./scripts/bench.sh --gate`;
- classifier changes: `python3 -m unittest tests/test_retrieval_benchmark_gate.py`;
- CLI, MCP, installation, or model-startup changes: `bash scripts/smoke-test.sh`;
- clean-environment or local-model work: follow `skills/mag-development/SKILL.md`.

CI consumes the same repository-owned benchmark classifier. Do not add an
independent path list to workflow YAML.

<!-- cairn:agent-guide-begin -->
## Cairn orientation

This project uses cairn to keep its architecture map in sync with code. Read
`.cairn/AGENTS.md` for full orientation, then follow
`.claude/skills/cairn-dev/SKILL.md` for the development loop.
<!-- cairn:agent-guide-end -->
