---
id: res.architecture-convergence-review-2026-08-07
nodes:
  - mag.runtime.entrypoints
  - mag.runtime.mcp
  - mag.runtime.memory.domain
  - mag.runtime.memory.retrieval
  - mag.runtime.memory.storage.sqlite
  - mag.quality.benchmarks
sources:
  - src.current-runtime-baseline
  - src.mag-agents-guide
  - src.source-tree-recon
date: 2026-08-07
---
# Architecture convergence and code-quality review

## Scope

This records the independent deep-module and code-quality review of
`main@3e883928baaa5881edd721c0bef0d1a0a988df84` and
`PR #414@2dae1ce05a364ac8d5ad0dd91ce53d04e1923e80`, reconciled against the
repository's accepted composition-root decision, Cairn work, open PRs, branches,
reviews, and exact-head CI.

The review found strong engineering discipline, deep retrieval implementation,
and unusually broad verification. The architectural migration had nevertheless
stopped before convergence: production core modules were compiled through both
the library and binary crate, while MCP constructed another runtime from storage.
Tests made that transition safe; they did not make it cheap to evolve.

## Product boundary clarification

MAG is operated through its CLI. The CLI is the canonical external application
contract for people, skills, automation, and local tools. MCP is an optional
transport convenience for hosts that require MCP.

That does not require MCP to spawn a CLI subprocess. The current in-process stdio
adapter should call the same library-owned runtime workflows, because shelling out
would add process, serialization, and error-mapping coupling. The architectural
requirement is one semantic implementation and CLI-aligned behavior, not one
process invocation path.

## Findings and disposition

### 1. One production module graph and one runtime — immediate correction

`main.rs` privately redeclared `memory_core` and `local_memory_runtime`, which
compiled a second production type graph beside `lib.rs`. `McpMemoryServer::new`
accepted `SqliteStorage` and constructed another `LocalMemoryRuntime`.

This caused exact-head PR #414 Clippy to report the new role-aware constructors as
dead only in the binary-private module copy while tests, benchmarks, smoke, npm,
and Python verification passed.

Disposition: PR #414 removes the private production module declarations, configures
storage and reranking before runtime construction, creates one
`Arc<LocalMemoryRuntime>`, injects it into MCP, and pins pointer identity. Do not
replace this correction with `#[allow(dead_code)]`.

### 2. Close the retrieval benchmark-gate gap — next independent slice

The engineering contract requires benchmark verification for retrieval, scoring,
reranking, and query-pipeline changes. CI watches several SQLite search paths but
does not explicitly watch:

- `src/memory_core/scoring.rs`;
- `src/memory_core/scoring_strategy.rs`;
- `src/memory_core/reranker.rs`;
- `src/memory_core/retrieval_strategy.rs`.

The configured `src/memory_core/scoring/**` path also does not match the actual
`scoring.rs` file.

Disposition: first correct the watched paths with a contract test. The stronger
follow-up is one repository-owned change classifier consumed by local verification
and CI so the written rule and workflow cannot drift independently.

### 3. Return atomic checkpoint outcomes — high-priority defect

Checkpoint numbering currently counts matching checkpoints and inserts in
separate operations. Concurrent writers can select the same number. CLI and MCP
then query again to infer the saved checkpoint number, which can observe another
writer's result.

Disposition: add a concurrent red test, make number allocation and insertion one
transaction with an enforceable uniqueness rule, and return a typed outcome
containing the memory ID and checkpoint number. CLI and MCP should serialize that
outcome without a second query.

### 4. Verify `doctor --fix` postconditions — high-priority defect

The command appears to apply fixes and then derive its final failure result from
the original pre-fix check collection.

Disposition: add a command-level regression in which all repairable failures are
fixed and the process exits successfully. Rerun or update checks after fixes;
unresolved failures must still produce a non-zero result.

### 5. Deepen `LocalMemoryRuntime` around cohesive workflows

The runtime remains broad and mostly delegates one method to SQLite or the
compatibility `Pipeline`. Removing it would still often amount to replacing
`runtime.foo()` with `storage.foo()`.

Disposition: deepen it incrementally where there is real coordination value:
atomic checkpoints, typed lifecycle outcomes, and duplicated multi-step CLI/MCP
workflows. Keep transport formatting outside. Do not introduce a broad replacement
`MemoryStore` or a third orchestration vocabulary.

### 6. Give MCP one contract source and a smaller maintenance burden

MCP currently manually synchronizes tool attributes, `TOOL_REGISTRY`,
`MINIMAL_TOOL_NAMES`, initialization instructions, generated protocol output, and
documentation. The same module describes the legacy surface as both 15 and 16
tools.

Disposition: derive advertisement, modes, counts, instructions, protocol output,
and docs from one owned contract. Prefer the four cohesive facade tools for new
integrations. Preserve legacy tools through their explicit compatibility gate,
then reassess full mode rather than expanding parallel behavior.

### 7. Collapse `EventType` policy when that area next changes

Adding one event type requires repeated edits for parsing/display, schema,
priority, TTL, retrieval weight, deduplication, supersession, and memory kind,
including a separate keep-in-sync list.

Disposition: consolidate policy into one exhaustive definition when event-policy
work is next selected. This is lower priority than correctness, verification, and
runtime convergence.

## Required sequence

1. Complete and merge PR #414 only after exact-head CI and Cairn verify the single
   module graph, one runtime, role-aware embedding boundary, and compatibility
   behavior.
2. Correct and test benchmark change classification.
3. Fix atomic checkpoint outcomes and `doctor --fix` postconditions in focused,
   TDD-backed slices.
4. Use those concrete workflows to deepen the runtime with typed outcomes.
5. Consolidate the MCP contract and simplify its advertised surface without
   breaking the compatibility gate.
6. Consolidate event policy when work next touches that domain.

## Constraints retained

- Do not promote `substrate` wholesale.
- Do not create a broad replacement `MemoryStore` interface.
- Do not remove the compatibility `Pipeline` before its release gate or change
  protected `processed: ` behavior incidentally.
- Do not split cohesive retrieval files solely because they are large.
- Continue the query/document model-role seam from PR #414; it represents real
  model variation and its compatibility adapter remains appropriately narrow.
