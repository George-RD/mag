# Benchmark Report
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

This document records the benchmark methodology and the latest measured outputs used by the README.

## Environment

- Date: `2026-03-28`
- Commit: `83abccf`
- Machine: `macOS aarch64, 12 CPU`
- OS: `macOS 26.3 (25D125)`
- Embedder: `bge-small-en-v1.5` (ONNX, 384-dim)

## Dataset Policy

- Benchmark datasets are fetched externally and cached under the active MAG root.
- Default cache root is `~/.mag/benchmarks/`.
- `--dataset-path` overrides the cache entirely.
- `--force-refresh` re-downloads the dataset.
- `cargo run --release --bin fetch_benchmark_data -- --dataset all` warms the cache without running the benchmarks.

## Safety

- Benchmarks do not use the normal production database.
- Official LongMemEval uses a fresh in-memory SQLite database per question.
- LoCoMo uses a fresh in-memory SQLite database per sample.
- The local benchmark uses an in-memory database by default and an explicit temp file only when `--file-backed` is requested.
- Model downloads may populate the active model cache under the resolved MAG root.

## Local LongMemEval-Style Benchmark

Command:

```bash
cargo run --release --bin longmemeval_bench -- --json
```

Result:

| Metric | Value |
| --- | --- |
| Dataset | `data/local_benchmark.json` |
| Seeded memories | `80` |
| Total questions | `100` |
| Correct | `98` |
| Overall | `98.0%` |
| Seeding time | `1538 ms` |
| Query time | `1013 ms` |
| Peak RSS | `335568 KB` |

| Category | Score |
| --- | --- |
| Information extraction | `20 / 20` |
| Multi-session reasoning | `20 / 20` |
| Temporal reasoning | `19 / 20` |
| Knowledge update | `19 / 20` |
| Abstention | `20 / 20` |

## Official LongMemEval_S

Command:

```bash
cargo run --release --bin longmemeval_bench -- --official --questions 10 --json
```

Result from the initial official sample rerun:

| Metric | Value |
| --- | --- |
| Dataset source | `https://huggingface.co/datasets/LIXINYI33/longmemeval-s/resolve/main/longmemeval_s_cleaned.json` |
| Cached path | `$HOME/.mag/benchmarks/longmemeval/longmemeval_s_cleaned.json` |
| Questions evaluated | `10 / 500` |
| Correct | `8` |
| Raw accuracy | `80.0%` |
| Total memories ingested | `5177` |
| Avg memories/question | `517.7` |
| Total duration | `455.74 s` |
| Avg query time | `36.3 ms` |
| Peak RSS | `559536 KB` |

Publication status:

- External fetch, cache reuse, explicit path override, and temporary cleanup support are implemented.
- The README does not yet publish a full `500`-question official score, because that full rerun was not completed in this batch window.

## LoCoMo10 Retrieval Benchmark

**Methodology:** For each of 10 LoCoMo conversations (~600 turns each), seed all turns as memories into a fresh in-memory database, then evaluate retrieval quality across 5 question categories. Primary metric is word-overlap recall (AutoMem-compatible); substring match and evidence recall are also reported.

Commands:

```bash
# Substring scoring (default)
cargo run --release --bin locomo_bench -- --json
# Word-overlap scoring (AutoMem-comparable)
cargo run --release --bin locomo_bench -- --scoring-mode word-overlap --json
# Fast iteration (2 samples, ~304 questions, ~15s)
cargo run --release --bin locomo_bench -- --samples 2 --scoring-mode word-overlap
```

Dataset: [`locomo10.json`](https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json) (10 conversations, 1986 questions)

### Run parameters

| Parameter | Value |
| --- | --- |
| Commit | `83abccf` |
| Date | `2026-03-28` |
| Samples evaluated | `10` |
| Questions evaluated | `1986` |
| Memories ingested | `5882` |
| Graph edges | `73002` (5610 PRECEDED_BY, 49806 RELATES_TO, 17586 related) |
| Top-k | `20` |
| Total duration | `~82 s` |
| Average query time | `7 ms` |
| Embedder | `bge-small-en-v1.5` (ONNX, 384-dim) |

### Results by category (word-overlap)

| Category | Questions | Word Overlap |
| --- | ---: | ---: |
| Single-Hop QA | 282 | `86.9%` |
| Temporal Reasoning | 321 | `85.0%` |
| Multi-Hop QA | 96 | `56.2%` |
| Open-Domain | 841 | `95.7%` |
| Adversarial | 446 | `92.6%` |
| **Overall** | **1986** | **`90.1%`** |

Overall Substring: `39.8%` | Overall Evidence Recall: `92.0%`

### Comparison (word-overlap, 10-sample)

| Category | MAG (10-sample) | AutoMem |
| --- | ---: | ---: |
| Single-Hop QA | `86.9%` | `79.8%` |
| Temporal Reasoning | `85.0%` | `85.1%` |
| Multi-Hop QA | `56.2%` | `50.0%` |
| Open-Domain | `95.7%` | `95.8%` |
| Adversarial | `92.6%` | `100.0%` |
| **Overall** | **`90.1%`** | **`90.5%`** |

AutoMem numbers are their published figures from the [LoCoMo paper](https://arxiv.org/abs/2402.18180) Table 2 (Recall column, LoCoMo-10 subset). MAG numbers use `--samples 10 --scoring-mode word-overlap --top-k 20`.

This is a retrieval-oriented benchmark, not a full generative evaluation. The README describes it that way intentionally.

### E2E LLM Evaluation Mode

The E2E (end-to-end) word-overlap mode combines LLM answer generation with word-overlap recall scoring. This mirrors AutoMem's evaluation pipeline: retrieve context, generate an LLM answer, then score the generated answer against the expected answer using word-overlap recall.

This gives a more realistic evaluation than retrieval-only word-overlap (which scores raw retrieved text) or LLM F1 (which uses token-level F1). Adversarial questions are scored via phrase-based detection (same as `llm-f1` mode).

Command:

```bash
# E2E word-overlap with OpenAI (requires OPENAI_API_KEY)
cargo run --release --bin locomo_bench -- --e2e --llm-judge --samples 2
# E2E word-overlap with local LM Studio
cargo run --release --bin locomo_bench -- --e2e --local --samples 2
# Equivalent explicit form
cargo run --release --bin locomo_bench -- --scoring-mode e2e-word-overlap --llm-judge --samples 2
```

| Category | MAG (E2E) | MAG (retrieval) | AutoMem |
| --- | ---: | ---: | ---: |
| Single-Hop QA | `25.0%` | `60.0%` | `79.8%` |
| Temporal Reasoning | `49.3%` | `87.6%` | `85.1%` |
| Multi-Hop QA | `5.8%` | `43.7%` | `50.0%` |
| Open-Domain | `54.1%` | `78.4%` | `95.8%` |
| Adversarial | `98.6%` | `74.4%` | `100.0%` |
| **Overall** | **`57.3%`** | **`75.3%`** | **`90.5%`** |

E2E numbers from `--e2e --llm-judge --samples 2` with gpt-4o-mini (2026-03-17). The LLM generates concise answers, so word-overlap recall is lower than retrieval-only for non-adversarial categories (fewer matching tokens). Adversarial jumps from 74.4% to 98.6% because the LLM correctly identifies absent information. This confirms the gap to AutoMem is primarily in retrieval quality, not evaluation methodology.

## Scale Benchmark

Command:

```bash
cargo run --release --bin scale_bench -- --max-scale 10000 --search-queries 50
```

Result:

| Scale | Store throughput | Avg store latency | Mean search | P95 | P99 | Recall@5 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1K | `75.6/s` | `13.22 ms` | `7.85 ms` | `16.80 ms` | `18.78 ms` | `100.0%` |
| 5K | `61.8/s` | `16.17 ms` | `7.41 ms` | `17.25 ms` | `30.01 ms` | `100.0%` |
| 10K | `53.3/s` | `18.75 ms` | `19.61 ms` | `42.56 ms` | `51.94 ms` | `100.0%` |

Degradation from `1K` to `10K`:

- Mean search latency: `7.85 ms -> 19.61 ms` (`2.5x`)
- P95 latency: `16.80 ms -> 42.56 ms` (`2.5x`)
- Recall@5: `100.0% -> 100.0%`

## omega-memory Comparison

Command:

Clone `omega-memory` locally first and point the comparison script at that checkout.

```bash
COMPARISON_REPO=/path/to/omega-memory
OMEGA_REPO="$COMPARISON_REPO" UV_CACHE_DIR=/tmp/uv-cache-mag uv run --project "$COMPARISON_REPO" python benches/python_comparison.py
```

Result:

| Metric | MAG | omega-memory |
| --- | ---: | ---: |
| Seeded memories | `80` | `80` |
| Correct | `98 / 100` | `90 / 100` |
| Overall | `98.0%` | `90.0%` |
| Seeding time | `1538 ms` | `986 ms` |
| Query time | `1013 ms` | `501 ms` |
| Peak RSS | `335568 KB` | `309024 KB` |

Category breakdown for `omega-memory`:

| Category | Score |
| --- | --- |
| Information extraction | `20 / 20` |
| Multi-session reasoning | `20 / 20` |
| Temporal reasoning | `15 / 20` |
| Knowledge update | `19 / 20` |
| Abstention | `16 / 20` |

Interpretation:

- MAG was more accurate on the shared local workload.
- omega-memory was faster on seeding and query time for that small benchmark.
- The comparison is intentionally reported as a measured tradeoff, not a blanket win claim.
