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
informed_by:
  - res.architecture-state-audit
  - res.production-composition-root-evaluation
refines:
  - dec.classify-current-runtime-boundaries
  - dec.sequence-architecture-before-llm-wiring
---
# Select an entrypoint-owned local runtime as the production composition root

## Decision

MAG will introduce one transport-independent local runtime facade, constructed
once by the process entrypoint and shared by CLI and MCP. The working name is
`LocalMemoryRuntime`; the implementation PR may choose a clearer name without
reopening this decision if the responsibilities remain the same.

The first runtime implementation will own or share:

- the selected embedder and later model-role services;
- one durable `SqliteStorage` instance;
- the current read, write, retrieval, lifecycle, and administration delegates;
- compatibility adapters required while public callers migrate.

It will initially delegate to current production behaviour. Creating the facade
is not permission to rewrite retrieval, stored content, schemas, or MCP responses.

## Boundary rules

1. CLI and MCP are transport/adaptation layers. They may assemble requests and
   format responses but may not grow independent memory semantics.
2. SQLite remains the durable local backend and the current implementation of
   most semantics. It is not the permanent application composition root.
3. The legacy `memory_core::Pipeline` remains a compatibility adapter only. Its
   visible `processed: ` stored-content behaviour is preserved through at least
   one released compatibility period and may change only through a separate,
   versioned decision with regression coverage.
4. The feature-gated `substrate` module is not selected as the production root.
   No new product behaviour will be implemented there independently.
5. Useful narrow substrate interfaces may be folded into the live runtime only
   with public-surface parity tests and, for retrieval/scoring/storage changes,
   the benchmark and evaluation gates. Duplicate interfaces are then deprecated
   rather than maintained as parallel extension points.
6. The runtime depends on domain/model/storage capabilities; local stdio and any
   later HTTP or hosted adapter depend on the runtime. The runtime must not depend
   on a daemon, cloud credential, or network transport.
7. Prefer narrow capability and model-role ports. The broad substrate
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

### Slice 3 — MCP migration

Route MCP tools through the same runtime capabilities and prove parity for both
full and minimal tool modes. Stdio remains the mandatory independent baseline.

### Slice 4 — model roles and evaluated intelligence

Introduce generation, extraction, embedding, and reranking as separate runtime
roles only after the local evaluation harness exists. Missing optional models
must produce explicit fallback or actionable failure rather than silently
changing semantics.

### Slice 5 — retirement

After all production callers use the runtime, retain the legacy adapters for one
released compatibility period unless an explicit versioned migration decision
sets a longer window. Then remove the legacy `Pipeline` construction and the
unselected substrate orchestrators/types in dedicated, evidence-backed PRs.
Tests or benchmarks that still depend on useful implementations move to the
selected runtime vocabulary first.

## Rollback

The initial slices are additive and make no data-format or schema changes. A
migration PR can restore a caller to its previous delegate without database
migration. Any later schema, embedding-space, or stored-content change requires
its own recovery and rollback design.

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

### Make the legacy `memory_core::Pipeline` the root

Rejected because it covers only part of the live surface, MCP does not use it,
and its placeholder processing behaviour is a compatibility concern rather than
a suitable orchestration contract.

## Trade-offs

The decision adds a facade and a temporary compatibility period. Poorly designed,
that facade could become another pass-through abstraction or a new monolith. The
mitigation is to keep the first implementation behaviour-neutral, expose narrow
capabilities, migrate one public command family at a time, and remove superseded
paths rather than preserving every historical abstraction.
