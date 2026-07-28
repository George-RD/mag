# MAG local-first development roadmap

<!-- Created: 2026-07-28 | Priority overlay for execution-roadmap.md -->

## Mission

Make MAG more useful than Hindsight for durable agent memory while remaining
fully functional with local inference on an ordinary modern computer. Cloud and
self-hosted service deployments remain supported, but they must not be required
to reach the core quality baseline.

There is no contradiction between local-first and service deployment:

- **Local mode** optimizes privacy, offline use, low marginal cost, and simple
  single-user operation.
- **Service mode** optimizes cross-device access, shared/team memory, centralized
  administration, and larger hardware.
- Both modes should implement the same model/runtime traits and memory semantics.

## Decisions now

1. **Default local generative model:** LFM2.5 1.2B Instruct.
2. **350M is not an automatic fallback:** it becomes eligible only after task-level
   evals show no material quality loss.
3. **Local quality is the primary benchmark:** remote frontier models are useful
   comparison ceilings, not hidden dependencies.
4. **Model roles remain separable:** generation/extraction, embedding, and reranking
   may use different models when that improves the quality/latency frontier.
5. **Current quick path is HTTP-local:** direct ONNX causal generation is the target
   architecture, not something the existing code already provides.

## Current state and blockers

- Embeddings run locally through ONNX (`bge-small-en-v1.5` by default).
- Retrieval has vector, BM25, graph, optional cross-encoder, and calibrated
  abstention work, but needs broader real-session evaluation.
- The optional LLM infrastructure supports OpenAI-compatible/Ollama endpoints and
  Anthropic, but it is not fully wired into the normal production ingestion path.
- `memory_core::Pipeline` and the newer `substrate` orchestration coexist. Adding
  intelligence to both would create drift; one production path must become the
  clear composition root.
- Direct ONNX causal generation needs model download/manifest handling, tokenizer
  and chat-template support, KV-cache generation, quantization selection, and
  hardware-provider configuration.

## Ordered development plan

### P0 — Establish the local 1.2B baseline

- Wire `LlmBackend` into the production ingestion/write path behind explicit
  configuration and a safe rule-based fallback.
- Add a local-only evaluation set for facts, entities, temporal references,
  relationships, decisions, questions, status, topic grouping, and contradictions.
- Record extraction validity, relationship precision/recall, task success, p50/p95
  latency, peak RAM, model load time, and generated tokens.
- Ensure a missing local model produces an actionable warning rather than silent
  success.

**Gate:** the 1.2B profile works end to end without cloud credentials and improves
memory usefulness over rule-only extraction.

### P1 — Remove the runtime architecture blocker

- Choose and document the production composition root: complete the `substrate`
  wiring or fold its trait boundaries into the current pipeline.
- Separate **model role**, **runtime**, and **transport**:
  - `GenerativeModel` / structured generation behavior;
  - `ModelRuntime` for ONNX, llama.cpp, or remote HTTP;
  - provider/transport adapters for local service and cloud modes.
- Add a model manifest with repository ID, revision, files, checksums, quantization,
  context limits, and supported hardware providers.
- Keep rule-based extraction and existing embeddings available when the generator
  is disabled or unavailable.

**Gate:** one code path configures local in-process and remote backends without
branching memory semantics.

### P2 — Add direct ONNX LFM2.5 1.2B inference

- Implement the in-process backend for
  `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`, starting with Q4.
- Support tokenizer/chat template, deterministic structured generation, bounded
  output, cancellation, warmup, and KV-cache reuse.
- Detect CPU/GPU/NPU execution providers and fall back predictably.
- Compare direct ONNX with Ollama/llama.cpp transport for quality, startup time,
  steady-state latency, RAM, and packaging complexity.

**Decision rule:** keep ONNX as the default only if its end-to-end operational
cost is lower without a material quality or portability regression.

### P3 — Calibrate retrieval and reranking

- Replace fixed global cutoffs with confidence calibrated from semantic score,
  score margin, lexical/semantic agreement, reranker score, query intent, and
  candidate diversity.
- Benchmark the existing cross-encoder within SessionStart latency limits.
- Evaluate local alternatives, including LFM2.5-Embedding-350M and
  LFM2.5-ColBERT-350M, against the current BGE embedder and reranker.
- Use dynamic result count and token budget rather than a fixed `limit`.
- Measure active injection versus passive retrieval on real agent-resumption tasks.

**Gate:** improve useful-recall and injection precision without exceeding local
latency/RAM budgets.

### P4 — Local memory intelligence

Use the 1.2B model for narrow, evaluated operations:

- canonical fact and decision extraction;
- entity normalization and relationship creation;
- memory clustering/topic grouping;
- contradiction and supersession proposals;
- consolidation summaries with provenance;
- query decomposition or rewrite only when retrieval evals justify it.

Never let generated structure overwrite raw memories. Store provenance, model
profile, prompt/schema version, confidence, and source memory IDs.

### P5 — Qualify the 350M speed tier

Run the same tasks through `LiquidAI/LFM2.5-350M-ONNX`. Promote only the tasks
whose quality stays within a predeclared tolerance of 1.2B. Likely candidates are
classification, simple tagging, and schema-constrained extraction; relationship
reasoning and consolidation should remain on 1.2B unless evidence says otherwise.

### P6 — Service and cross-device mode

- Reuse the same runtime/model interfaces behind HTTP or hosted workers.
- Add authentication, encryption, synchronization, tenancy, and conflict handling
  separately from memory-quality logic.
- Preserve an entirely local single-binary mode with no service dependency.

## Next pull requests

1. **Wire local LLM into production ingestion** with explicit enable/disable and
   observable fallback.
2. **Create the local memory-intelligence eval harness** and a small versioned
   dataset before adding more extraction behavior.
3. **Resolve Pipeline versus substrate composition** and document the chosen path.
4. **Prototype direct ONNX LFM2.5 generation** behind a feature flag and model
   manifest.
5. **Build the retrieval/reranker matrix** and calibrate abstention/injection.
6. **Add provenance-preserving relationship and clustering proposals.**
7. **Evaluate 350M task routing** only after the 1.2B baseline is stable.

## Baseline scorecard

Every model/runtime change should report:

- extraction schema validity;
- fact/entity/relationship precision and recall;
- retrieval Recall@5/10 and MRR;
- abstention accuracy and injected-memory precision;
- active-injection task-success delta;
- p50/p95 cold and warm latency;
- peak RAM and on-disk model size;
- offline success rate after model installation;
- quality delta versus the 1.2B local reference.
