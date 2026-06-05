# MAG vs Hindsight: Honest Competitive Comparison
<!-- Generated: 2026-06-05 | Status: Phase 0 baseline measurement -->

> **Purpose:** Establish an apples-to-apples comparison between MAG and Hindsight on the same E2E evaluation protocol before any algorithmic changes. This document is updated as benchmarks complete.

## Honest Current State

**MAG** is a local-first, private, zero-cost agent memory system (SQLite + ONNX embeddings + FTS5 + graph edges) with 19 MCP tools. It runs entirely on-device with no mandatory LLM or cloud services.

**Hindsight** (arXiv 2512.12818, 15k+ stars) is a cloud-backed memory system with 4 memory networks (world facts, experiences, entity summaries, evolving beliefs) + retain/recall/reflect, using an LLM in the loop for extraction and reflection.

### Critical Measurement Gap (Fixed in Phase 0)

Prior to this document, MAG's published numbers were **not comparable** to Hindsight's:

| Metric | MAG (before) | Hindsight | Problem |
|--------|-------------|-----------|---------|
| LongMemEval | 91.5% (retrieval-only) | **91.4%** | MAG judged *retrieved memories*, Hindsight judges *generated answers* |
| LoCoMo | 91.5% (word-overlap) | **89.61%** | MAG word-overlap recall on retrieved text; Hindsight E2E answer generation |
| E2E LongMemEval | **Not published** | 91.4% | No E2E number existed for MAG |
| E2E LoCoMo | **Not published** | 89.61% | No E2E number existed for MAG |

**Phase 0 fixes this** by adding true E2E evaluation: retrieve top-k → LLM generates answer → judge generated answer vs expected.

## E2E Methodology (Standard Protocol)

To match the Hindsight paper's evaluation:
1. **Retrieve** top-k memories using MAG's search pipeline (advanced_search → RRF → scoring → abstention gate)
2. **Generate** an answer using a fixed backbone LLM with retrieved context as input
3. **Judge** the generated answer against the expected answer using the standard LongMemEval/LoCoMo judge

This is a harder test than retrieval-only because the LLM must synthesize a correct answer from the retrieved context, not just retrieve relevant text.

### Backbone LLM Tiers

Following Hindsight's multi-tier evaluation:

| Tier | Model | Purpose |
|------|-------|---------|
| Local | ~20B parameter model via Ollama/LM Studio | Cost-free, private, reproducible local baseline |
| API | GPT-4o-class (gpt-4o / gpt-4o-mini) | Cloud-backed upper-bound comparable to Hindsight |

### Commands

**LongMemEval_S E2E (500 questions):**
```bash
cargo run --release --bin longmemeval_bench -- --official --e2e --local --llm-url http://localhost:1234/v1/chat/completions --judge-model qwen3.5-9b-optiq

# API tier (gpt-4o-mini)
cargo run --release --bin longmemeval_bench -- --official --e2e --judge-model gpt-4o-mini

# API tier (gpt-4o)
cargo run --release --bin longmemeval_bench -- --official --e2e --judge-model gpt-4o
```

**LoCoMo E2E:**
```bash
# Local tier
cargo run --release --bin locomo_bench -- --e2e --local --samples 10 --llm-model qwen3.5-9b-optiq

# API tier
cargo run --release --bin locomo_bench -- --e2e --llm-judge --samples 10 --llm-model gpt-4o-mini
```

## Results

### LongMemEval_S E2E

| Tier | Model | Questions | Raw Accuracy | Task-Averaged | Date | Notes |
|------|-------|-----------|-------------|---------------|------|-------|
| API | gpt-4o-mini | 10 (sample) | TBD | TBD | 2026-06-05 | Preliminary run in progress |
| API | gpt-4o-mini | 500 (full) | — | — | — | Requires ~6hrs + API budget |
| Local | TBD | 500 (full) | — | — | — | Requires local model setup |

### LoCoMo E2E

| Tier | Model | Samples | Overall | 1-Hop | Temporal | Multi-Hop | Open | Adv | Date | Notes |
|------|-------|---------|---------|-------|----------|-----------|------|-----|------|-------|
| API | gpt-5.4 | 2 | 82.8% | 77.8% | 82.6% | 71.4% | 86.2% | 85.7% | 2026-03-29 | Prior retrieval E2E (word-overlap on generated answers) |
| API | gpt-4o-mini | 2 | 76.0% | — | — | — | — | — | 2026-03-29 | Prior retrieval E2E |
| API | gpt-4o | 2 | 90.3% | — | — | — | — | — | 2026-03-29 | Prior retrieval E2E |

> **Note:** The LoCoMo E2E numbers above are from the existing `--e2e` mode in `benches/locomo/`, which uses word-overlap scoring on LLM-generated answers. The LongMemEval E2E mode is newly added in Phase 0 and uses the official LLM judge on generated answers.

### MAG's Structural Moat (Uncontested)

These dimensions are orthogonal to benchmark scores and represent MAG's durable differentiation:

| Dimension | MAG | Hindsight |
|-----------|-----|-----------|
| Cost | **$0** runtime | Requires OpenAI API key ($) |
| Privacy | **100% on-device** | Cloud LLM calls for extraction/reflection |
| Deployment | **Single Rust binary** | Python + Postgres + Docker + cloud LLM |
| Offline | **Works without internet** | Requires internet for LLM |
| Latency | **~2-20ms search** | Network round-trip to LLM |
| Integration | **19 MCP tools, native** | Custom API |
| Mandatory LLM | **None** | Required for core functionality |

## Benchmark-Driven Roadmap

Phase 0 establishes honest measurement. Subsequent phases close the quality gap:

1. **Phase 1:** Optional LLM backend (OpenAI/Anthropic/Ollama) — off by default
2. **Phase 2:** LLM-grade ingestion extraction (facts, entities, relationships, temporal normalization)
3. **Phase 3:** Reflection/learning layer (`reflect` verb — entity summaries, evolving beliefs)
4. **Phase 4:** Close retrieval-quality gaps driven by benchmark results
5. **Phase 5:** Positioning — "local-first agent memory: retain, recall, reflect"

## CI / Regression Guard

E2E benchmarks are run manually on significant releases due to cost/time. The benchmark harness supports:
```bash
# Quick gate (10 questions, fast)
cargo run --release --bin longmemeval_bench -- --official --questions 10 --e2e

# Full validation (500 questions, expensive)
cargo run --release --bin longmemeval_bench -- --official --e2e --judge-model gpt-4o-mini
```

To prevent silent rot, the E2E code paths are exercised in CI by:
- Compiling `--bin longmemeval_bench` and `--bin locomo_bench` with all feature combinations
- Unit tests for `generate_answer` and `llm_judge_eval` using a mock/deterministic backend (when `test-helpers` feature is enabled)

## References

- Hindsight paper: [arXiv 2512.12818](https://arxiv.org/abs/2512.12818)
- LongMemEval: [arXiv 2407.11963](https://arxiv.org/abs/2407.11963)
- LoCoMo: [arXiv 2402.17753](https://arxiv.org/abs/2402.17753)
- MAG benchmark code: `benches/longmemeval/`, `benches/locomo/`
