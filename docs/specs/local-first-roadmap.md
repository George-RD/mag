# MAG local-first development roadmap

<!-- Revised: 2026-07-28 | Status is held in Cairn todo artefacts -->

## Mission

Make MAG more useful than Hindsight for durable agent memory while remaining
fully functional with local inference on an ordinary modern computer. Cloud and
self-hosted deployments remain supported, but they are not prerequisites for the
core quality baseline.

Local and service operation are complementary deployment modes:

- **Local mode** prioritizes privacy, offline use, low marginal cost, and simple
  single-user operation.
- **Service mode** adds cross-device access, team memory, centralized
  administration, and larger hardware.
- Both implement the same model-role, memory, provenance, and retrieval contracts.

## How this roadmap is executed

This document records mission, sequence, trade-offs, and exit gates. It is not a
second task tracker. Current status is stored in small node-linked files under
`meta/todos/` and queried through Cairn:

```bash
cairn status
cairn next
cairn todos
cairn brief todo.audit-current-architecture-and-dead-code
cairn context --scope mag.runtime.memory
cairn rationale mag.runtime.memory
```

Decisions and research are similarly bounded:

```bash
cairn decisions mag.runtime.memory
cairn research mag.runtime.memory
cairn bundle mag.runtime.memory.models
```

A todo changes state with `cairn todo set <slug> <status>`. Do not add checkboxes
here; doing so would create two status sources.

## Decisions in force

1. Cairn is the queryable development-context and work-status layer.
2. LFM2.5 1.2B Instruct is the local generative quality reference.
3. LFM2.5 350M is eligible only after task-level parity against 1.2B.
4. Generation, embedding, and reranking are separate model roles.
5. Current generation is HTTP-local; direct ONNX generation is a candidate
   runtime, not a preselected outcome.
6. Derived facts, relationships, clusters, and summaries never overwrite raw
   memories and retain provenance.
7. Cleanup is evidence-based; stale recon reports are inputs, not authority.

## Contradictions resolved

### Production wiring versus architecture boundary

The previous roadmap identified the coexistence of `memory_core::Pipeline` and
`substrate` as a blocker but listed production LLM wiring before resolving it.
The corrected dependency order is architecture audit, composition decision,
evaluation harness, then production wiring.

### Local-first versus hosted service

There is no product contradiction. The local baseline is mandatory; hosted mode
is an optional deployment and synchronization layer over the same semantics.

### “Clean codebase” versus “faulty architecture”

Historical reports found little unreachable code while also identifying large,
concentrated modules. Those statements can both be true. The current priority is
live semantic duplication and composition ambiguity, not deleting code because a
file is large or feature-gated.

## Ordered development plan

### P0 — Establish the architecture and evaluation foundation

- Onboard and curate Cairn against the actual repository.
- Audit current callers, feature flags, fallback paths, tests, and benchmarks.
- Choose the production composition root and define the migration/removal path.
- Separate model role, runtime, and transport in the chosen architecture.
- Build the local memory-intelligence evaluation harness and versioned dataset.

**Gate:** one documented production path, no unexplained duplicate semantics,
bounded Cairn queries for connected context, and a reproducible local scorecard.

### P1 — Establish the local LFM2.5 1.2B production baseline

- Wire `LlmBackend` into the chosen production ingestion/write path.
- Require explicit enable/disable behavior and observable fallback.
- Evaluate facts, entities, temporal references, relationships, decisions,
  questions, status, grouping, contradictions, and provenance.
- Ensure a missing local model produces an actionable warning.

**Gate:** the 1.2B profile works end to end without cloud credentials and improves
memory usefulness over rule-only extraction within local latency/RAM budgets.

### P2 — Evaluate direct local inference

- Prototype `LiquidAI/LFM2.5-1.2B-Instruct-ONNX` behind the model-runtime boundary.
- Add manifest, revision, checksums, quantization, tokenizer/chat template,
  bounded structured output, cancellation, warmup, and KV-cache reuse.
- Detect CPU/GPU/NPU execution providers and fall back predictably.
- Compare direct ONNX with Ollama/llama.cpp transport for quality, startup,
  steady-state latency, RAM, disk size, and packaging complexity.

**Decision rule:** direct ONNX becomes default only if it lowers end-to-end
operational cost without a material quality or portability regression.

### P3 — Calibrate retrieval and reranking

- Calibrate confidence from semantic score, score margin, lexical/semantic
  agreement, reranker score, query intent, and candidate diversity.
- Benchmark the existing cross-encoder within session-start latency limits.
- Evaluate local embedding/reranker alternatives against the current BGE path.
- Use dynamic result count and token budget.
- Measure active injection versus passive retrieval on agent-resumption tasks.

**Gate:** useful recall and injection precision improve without exceeding local
latency, RAM, and context budgets.

### P4 — Add local memory intelligence

Use the 1.2B model for narrow, evaluated operations:

- canonical fact and decision extraction;
- entity normalization and relationship proposals;
- memory clustering and topic grouping;
- contradiction and supersession proposals;
- consolidation summaries with provenance;
- query decomposition only when retrieval evaluations justify it.

**Gate:** derived structures improve downstream task success, retain full source
lineage, and can be invalidated or regenerated independently of raw memories.

### P5 — Qualify the 350M speed tier

Run the same task-level evaluations with LFM2.5 350M. Promote only tasks within a
predeclared tolerance of the 1.2B reference. Classification and constrained
tagging are likely candidates; relationship reasoning and consolidation remain
on 1.2B unless evidence says otherwise.

### P6 — Add service and cross-device mode

- Reuse the same model/runtime interfaces behind hosted workers or HTTP.
- Add authentication, encryption, synchronization, tenancy, and conflict handling
  separately from memory-quality logic.
- Preserve an entirely local single-binary mode with no service dependency.

## Baseline scorecard

Every model, runtime, retrieval, or intelligence change reports:

- extraction schema validity;
- fact/entity/relationship precision and recall;
- retrieval Recall@5/10, MRR, and abstention accuracy;
- injected-memory precision and active-injection task-success delta;
- p50/p95 cold and warm latency;
- peak RAM, on-disk model size, and model load time;
- generated tokens and context tokens injected;
- offline success after model installation;
- quality delta versus the LFM2.5 1.2B local reference.
