# MAG local-first development roadmap

<!-- Revised: 2026-07-29 | Status is held in Cairn todo artefacts -->

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
4. Generation, dense embedding, cross-encoding, and late-interaction reranking
   are separate model roles.
5. Current generation is HTTP-local; direct ONNX generation is a candidate
   runtime, not a preselected outcome.
6. Derived facts, relationships, clusters, and summaries never overwrite raw
   memories and retain provenance.
7. Cleanup is evidence-based; stale recon reports are inputs, not authority.
8. LFM2.5 Embedding is an optional multilingual dense-retrieval candidate;
   BGE remains the compact default until MAG-specific evaluation says otherwise.
9. LFM2.5 ColBERT is evaluated first as a bounded reranker. A full multi-vector
   index requires a separate evidence-backed storage decision.
10. Retriever and generative fine-tuning follows stable architecture and
    held-out evaluations. Evaluation runs begin collecting training evidence
    before training begins.

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

### One model family versus one model role

Using LFM2.5 checkpoints for generation, dense retrieval, and late interaction
can simplify packaging and future specialization, but the checkpoints have
different heads, outputs, runtime needs, and licences. MAG therefore shares
model-profile and evaluation infrastructure without treating them as one
interchangeable loaded model.

## Dependency and parallelism

P0 is the common gate. After it:

- P1 and P2 form the generative-runtime track.
- P3 is an independent retrieval track and may proceed once the P0 model
  boundary and evaluation harness exist; it does not wait for direct generative
  inference.
- P4 depends on the P1 production baseline.
- P5 qualifies smaller generative routing only after P1 and relevant P4 tasks.
- P6 depends on stable selected behavior from P3 through P5.

## Ordered development plan

### P0 — Establish the architecture and evaluation foundation

- Onboard and curate Cairn against the actual repository.
- Audit current callers, feature flags, fallback paths, tests, and benchmarks.
- Choose the production composition root and define the migration/removal path.
- Separate model role, runtime, and transport in the chosen architecture.
- Define model profiles with query/document roles, embedding-space identity,
  dimensions, pooling, runtime, quantization, checksums, and licence metadata.
- Build the local memory-intelligence evaluation harness and versioned dataset.

**Gate:** one documented production path, no unexplained duplicate semantics,
bounded Cairn queries for connected context, explicit model-profile contracts,
and a reproducible local scorecard.

### P1 — Establish the local LFM2.5 1.2B production baseline

- Wire `LlmBackend` into the chosen production ingestion/write path.
- Require explicit enable/disable behavior and observable fallback.
- Evaluate facts, entities, temporal references, relationships, decisions,
  questions, status, grouping, contradictions, and provenance.
- Ensure a missing local model produces an actionable warning.

**Gate:** the 1.2B profile works end to end without cloud credentials and improves
memory usefulness over rule-only extraction within local latency/RAM budgets.

### P2 — Evaluate direct local generative inference

- Prototype `LiquidAI/LFM2.5-1.2B-Instruct-ONNX` behind the model-runtime boundary.
- Add manifest, revision, checksums, quantization, tokenizer/chat template,
  bounded structured output, cancellation, warmup, and KV-cache reuse.
- Detect CPU/GPU/NPU execution providers and fall back predictably.
- Compare direct ONNX with Ollama/llama.cpp transport for quality, startup,
  steady-state latency, RAM, disk size, and packaging complexity.

**Decision rule:** direct ONNX becomes default only if it lowers end-to-end
operational cost without a material quality or portability regression.

### P3 — Calibrate retrieval and qualify LFM2.5 retrievers

- Complete role-aware retriever profiles and the explicit re-embedding migration
  tracked by issue #89.
- Establish the current BGE plus MiniLM/no-reranker baseline on the versioned
  evaluation set.
- Evaluate `LFM2.5-Embedding-350M` as a 1024-dimensional multilingual and
  cross-lingual first-stage profile with correct query/document semantics.
- Evaluate `LFM2.5-ColBERT-350M` first as a bounded top-N reranker through the
  existing reranker boundary.
- Compare on-demand and content-hash-cached ColBERT document embeddings.
- Run the full matrix: BGE baseline, LFM dense, BGE plus ColBERT, and LFM dense
  plus ColBERT.
- Include English, Arabic, and English/Arabic cross-lingual retrieval.
- Calibrate confidence from semantic score, margin, lexical/semantic agreement,
  reranker score, query intent, and candidate diversity per model profile.
- Use dynamic result count and token budget only after calibration.
- Measure active injection versus passive retrieval on agent-resumption tasks.

**Gate:** a profile improves useful recall, injection precision, or downstream
task success without exceeding local latency, RAM, disk/index, migration, and
licence budgets. BGE remains the default without that evidence. A full ColBERT
multi-vector index is considered only through a later decision if bounded
reranking leaves first-stage recall as the measured bottleneck.

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

### P5 — Qualify the 350M generative speed tier

Run the same task-level evaluations with LFM2.5 350M. Promote only tasks within a
predeclared tolerance of the 1.2B reference. Classification and constrained
tagging are likely candidates; relationship reasoning and consolidation remain
on 1.2B unless evidence says otherwise.

### P6 — Prepare task-specific LFM specialization

- Freeze the selected production composition, retrieval profiles, candidate
  semantics, memory schemas, and evaluation datasets before training.
- Retain provenance-linked query, positive, hard-negative, wrong-time,
  superseded/current, feedback, and downstream task-outcome examples.
- Evaluate dense retriever fine-tuning first when retrieval evidence supports it.
- Fine-tune ColBERT only if its bounded reranker role is selected.
- Fine-tune 350M and 1.2B generative checkpoints only for narrow tasks with
  separate quality references.
- Version datasets, prompts, adapters, model lineage, licence metadata, and
  rollback behavior.

**Gate:** a fine-tuned profile improves held-out downstream task success over the
untuned production baseline without hiding an architectural or algorithmic
failure.

### P7 — Add service and cross-device mode

- Reuse the same model/runtime interfaces behind hosted workers or HTTP.
- Add authentication, encryption, synchronization, tenancy, and conflict handling
  separately from memory-quality logic.
- Preserve an entirely local single-binary mode with no service dependency.

## Baseline scorecard

Every model, runtime, retrieval, or intelligence change reports:

- extraction schema validity;
- fact/entity/relationship precision and recall;
- retrieval Recall@5/10, MRR, and abstention accuracy;
- English, Arabic, and English/Arabic cross-lingual retrieval where relevant;
- injected-memory precision and active-injection task-success delta;
- p50/p95 cold and warm latency;
- peak RAM, on-disk model size, database/index growth, and model load time;
- re-embedding or migration time and recovery behavior when vector spaces change;
- generated tokens and context tokens injected;
- offline success after model installation;
- quality delta versus the relevant untuned local reference.
