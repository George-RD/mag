---
id: dec.select-local-runtime-composition-root
nodes:
  - mag.runtime.entrypoints
  - mag.runtime.mcp
  - mag.runtime.memory.domain
  - mag.runtime.memory.models
  - mag.runtime.memory.retrieval
  - mag.runtime.memory.storage.sqlite
  - mag.runtime.substrate
status: accepted
date: 2026-07-29
revisit_triggers:
  - "A second production storage backend requires atomic runtime substitution"
  - "Service mode cannot reuse the local runtime contract without semantic duplication"
  - "Parity and resource evaluations show the current substrate composition is materially superior"
  - "The local model-role boundary cannot be introduced without changing the facade responsibilities"
  - "A required MCP capability cannot remain a thin adaptation of the CLI-owned application contract"
informed_by:
  - res.architecture-state-audit
  - res.production-composition-root-evaluation
  - res.architecture-convergence-review-2026-08-07
refines:
  - dec.classify-current-runtime-boundaries
  - dec.sequence-architecture-before-llm-wiring
---
# Select a CLI-first, entrypoint-owned local runtime as the production composition root

## Decision

MAG is operated primarily through its CLI. The CLI is the canonical user-facing
application contract used by people, skills, automation, and local tooling.

The process entrypoint constructs one transport-independent local runtime facade
and shares it with the CLI command handlers and any optional transport adapter.
The working name is `LocalMemoryRuntime`; the implementation may choose a clearer
name without reopening this decision if the responsibilities remain the same.

MCP is a transport convenience, not a second application surface. The in-process
stdio adapter receives the entrypoint-owned runtime and adapts MCP requests to the
same application capabilities. It must not construct its own runtime, own storage,
or implement independent workflows. It should call the shared in-process
capabilities rather than shelling out to the CLI, because a subprocess boundary
would add JSON, process-lifecycle, and error-mapping coupling without changing the
canonical contract.

The runtime owns or shares:

- the selected embedder and later model-role services;
- one durable `SqliteStorage` instance;
- the current read, write, retrieval, lifecycle, and administration workflows;
- compatibility adapters required while public callers migrate.

It initially delegates to current production behaviour. Creating or deepening the
runtime is not permission to rewrite retrieval, stored content, schemas, CLI
output, or MCP responses without explicit compatibility evidence.

## Boundary rules

1. The CLI is MAG's canonical operating surface. Product documentation, skills,
   examples, and automation should prefer CLI commands unless an integration
   specifically requires another transport.
2. MCP is an optional transport/adaptation layer. It may validate transport
   requests and format responses but may not grow independent memory semantics,
   storage ownership, or orchestration.
3. The binary consumes core and runtime modules through the library crate. It may
   not privately redeclare production copies of `memory_core` or
   `local_memory_runtime`.
4. The process entrypoint configures model and storage adapters once, constructs
   one `LocalMemoryRuntime`, and passes that same runtime to the selected adapter.
5. SQLite remains the durable local backend and the current implementation of
   most semantics. It is not the permanent application composition root.
6. The legacy `memory_core::Pipeline` remains a compatibility adapter only. Its
   visible `processed: ` stored-content behaviour is preserved through at least
   one released compatibility period and may change only through a separate,
   versioned decision with regression coverage.
7. The feature-gated `substrate` module is not selected as the production root.
   No new product behaviour will be implemented there independently.
8. Useful narrow substrate interfaces may be folded into the live runtime only
   with public-surface parity tests and, for retrieval/scoring/storage changes,
   the benchmark and evaluation gates. Duplicate interfaces are then deprecated
   rather than maintained as parallel extension points.
9. The runtime depends on domain/model/storage capabilities; local stdio and any
   later HTTP or hosted adapter depend on the runtime. The runtime must not depend
   on a daemon, cloud credential, or network transport.
10. Prefer narrow capability and model-role ports. The broad substrate
    `MemoryStore` supertrait will not become the root contract because it mirrors
    storage, search, graph, lifecycle, administration, and SQLite-oriented
    candidate operations in one interface.

## Migration and compatibility plan

### Slice 1 — additive facade and parity harness

Add the runtime as a thin delegate over the current constructors. Pin store,
retrieve, search, advanced-search, and MCP-visible response behaviour. No caller
is removed and no schema changes are allowed.

### Slice 2 — CLI migration by command family

Move write commands first while preserving `processed: ` exactly, then basic
read commands, then extended administration and retrieval commands. Each slice
keeps the previous delegate easy to restore. Search/scoring slices run the
benchmark gate.

### Slice 3 — one module graph and optional MCP transport

Compile core/runtime production code once through the library, configure storage
and model adapters once, and pass the entrypoint-owned runtime into MCP. Route MCP
tools through the same runtime workflows and prove parity for both full and
minimal tool modes. Stdio remains the mandatory independent baseline. Contract
advertisement, counts, instructions, and generated protocol documentation should
converge on one owned source rather than remain manually synchronized.

### Slice 4 — model roles and evaluated intelligence

Introduce generation, extraction, embedding, and reranking as separate runtime
roles only after the local evaluation harness exists. Missing optional models
must produce explicit fallback or actionable failure rather than silently
changing semantics.

### Slice 5 — deepen workflows and retire compatibility paths

Move cohesive, multi-step memory workflows and typed outcomes behind the runtime
as evidence justifies them. Prioritize atomic operations and duplicated adapter
workflows; do not create a broad replacement store interface.

After all production callers use the runtime, retain the legacy adapters for one
released compatibility period unless an explicit versioned migration decision
sets a longer window. Then remove the legacy `Pipeline` construction and the
unselected substrate orchestrators/types in dedicated, evidence-backed PRs.
Tests or benchmarks that still depend on useful implementations move to the
selected runtime vocabulary first.

## Convergence correction — 2026-08-07

The command-family migrations routed behavior through `LocalMemoryRuntime`, but
the binary still privately compiled `memory_core` and `local_memory_runtime`, and
`McpMemoryServer` constructed a second runtime from storage. This was a
transitional implementation, not the selected architecture. PR #414 removes the
private production module copies, configures reranking before runtime
construction, passes one `Arc<LocalMemoryRuntime>` into MCP, and adds a regression
test that pins runtime identity.

## Rollback

The migration changes no data format or schema. A workflow implementation can be
restored behind the runtime without database migration. Any later schema,
embedding-space, or stored-content change requires its own recovery and rollback
design.

## Alternatives rejected

### Promote the current substrate wholesale

Rejected because it lacks public parity and comparative quality/resource
evidence, omits live advanced-search behaviours, duplicates existing retrieval
abstractions, and uses a broad store interface that carries SQLite-oriented
operations. Promotion would combine an architectural migration with a retrieval
rewrite.

### Keep direct SQLite and legacy-pipeline construction permanently

Rejected because semantics remain scattered across entrypoint branches, MCP,
SQLite, and the compatibility adapter. Model roles and service mode would then be
likely to create further command-specific paths.

### Make MCP invoke the CLI process for every request

Rejected for the current in-process stdio adapter. It would preserve the CLI as a
process boundary but introduce subprocess lifecycle, serialization, and error
translation while duplicating no less application behavior. The shared runtime
keeps one semantic implementation while the CLI remains the canonical external
contract.

### Make the legacy `memory_core::Pipeline` the root

Rejected because it covers only part of the live surface, MCP does not use it,
and its placeholder processing behaviour is a compatibility concern rather than
a suitable orchestration contract.

## Trade-offs

The decision adds a facade and a temporary compatibility period. Poorly designed,
that facade could remain a pass-through abstraction or become a new monolith. The
mitigation is to deepen it only around cohesive workflows, return typed outcomes,
preserve transport formatting outside, and remove superseded paths rather than
preserving every historical abstraction.

Keeping MCP in-process means its tool schema remains another transport contract
to maintain. The mitigation is to keep the advertised MCP surface small, derive
its representations from one contract source, and treat CLI behavior as the
reference when parity questions arise.
