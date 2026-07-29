---
id: res.production-composition-root-evaluation
nodes:
  - mag.runtime.entrypoints
  - mag.runtime.mcp
  - mag.runtime.memory.domain
  - mag.runtime.memory.retrieval
  - mag.runtime.memory.storage.sqlite
  - mag.runtime.substrate
sources:
  - src.current-runtime-baseline
  - src.mag-agents-guide
  - src.source-tree-recon
date: 2026-07-29
---
# Production composition-root evaluation

## Question

Should MAG promote the feature-gated `substrate` module wholesale, keep the
current scattered SQLite and legacy-pipeline construction, or introduce one
entrypoint-owned local runtime over the live implementation and fold in only
proven boundaries?

The comparison uses the criteria required by the roadmap: public CLI/MCP parity,
retrieval quality, latency, memory use, operational complexity, testability, and
reversibility. A path being compiled, tested, or benchmarked is not treated as
production wiring.

## Evidence from the live path

`main.rs` constructs one embedder and one `SqliteStorage`, clones that storage
into MCP and extended CLI operations, and also wraps it in the legacy
`memory_core::Pipeline` for ingest/process and a subset of basic reads. The MCP
server is concrete over `SqliteStorage`.

The live SQLite advanced-search path already owns materially more behaviour than
the candidate substrate pipeline:

- temporal-query expansion and filter derivation;
- intent classification and intent-specific weights;
- query and hot-tier caches;
- dynamic candidate oversampling;
- embedding computation and dual vector/FTS collection;
- optional cross-encoder scores;
- fusion, score refinement, graph and entity enrichment;
- abstention, deduplication, sub-query decomposition, and cache merging.

Moving production callers away from that path without a parity harness would be
a retrieval rewrite, not an architectural rewire.

## Evidence from the candidate substrate

The `substrate` feature is constructed by tests and benchmark binaries only. Its
current evidence proves that an FTS-only pipeline returns a result and that a
write pipeline can reuse a precomputed embedding. It does not prove public CLI or
MCP parity, current advanced-search parity, data migration safety, real-model
resource use, or downstream task quality.

Its synthetic benchmark uses generated content and a placeholder embedder and
reports only the substrate pipeline's latency. It does not compare against the
live advanced-search path or measure peak RAM, model loading, database growth, or
quality.

The proposed `MemoryStore` boundary mirrors almost the entire existing capability
surface and also exposes SQLite-oriented candidate collection and enrichment
operations. It therefore relocates coupling rather than providing a narrow
composition seam. The repository already contains a second
`memory_core::retrieval_strategy` family and query context, so wholesale
promotion would retain two overlapping orchestration vocabularies during the
highest-risk migration period.

## Options

| Criterion | Promote current substrate wholesale | Keep direct construction permanently | Entry-point-owned local runtime over live path |
|---|---|---|---|
| CLI/MCP parity | Not demonstrated | Preserved, but remains inconsistent | Preserved by delegation, then pinned per migrated slice |
| Retrieval quality | Unknown against the live path | Preserved | Preserved initially; changes remain benchmark/eval gated |
| Latency and RAM | No comparative evidence | Current baseline | Initial facade should be negligible; algorithmic changes measured separately |
| Operational complexity | Adds feature gates, duplicate traits, and broad migration | Low short-term, high long-term coordination cost | One construction point; temporary adapters are explicit and bounded |
| Testability | Isolated unit/integration coverage only | Existing tests, but scattered callers | Existing tests plus public-surface parity tests around one facade |
| Reversibility | Wholesale caller migration is costly to reverse | No migration, but ambiguity persists | Each command family can revert to the previous delegate without schema change |
| Local/service separation | Possible in principle, not assembled | Transport and semantics remain easy to mix in entrypoints | Runtime is transport-independent; stdio and later service adapters depend on it |

## Conclusion

Do not promote the current `substrate` module wholesale and do not make either
`SqliteStorage` or the legacy `Pipeline` the permanent application root.

Introduce one entrypoint-owned local runtime facade, constructed once and shared
by CLI and MCP. Its first implementation wraps the existing `SqliteStorage` and
model components and delegates without changing stored content, retrieval, or
public responses. This creates the seam needed for model roles and optional
service adapters while preserving the verified local stdio path.

Fold narrow, demonstrated interfaces into that live path only when a migration
slice needs them. Prefer the existing capability traits and live retrieval
boundaries over the monolithic substrate `MemoryStore`. Once callers and useful
implementations have moved, deprecate and remove the duplicate substrate
orchestrators and types.

## Uncertainty and evidence that would change the decision

No claim is made that a facade improves latency or memory use; the initial target
is behavioural neutrality and near-zero orchestration overhead. The decision
should be revisited if a second production backend requires atomic substitution,
if service mode cannot reuse the local runtime contract, or if a parity and
resource evaluation shows the current substrate composition materially
outperforms the live path without losing behaviour.
