# Benchmark Report

This document records the benchmark methodology and the latest measured outputs used by the README.

## Environment

- Date: `2026-03-12`
- Commit: `66a9e3e97e0e65328864c4699ad6b14ccf8a24ae`
- Machine: `macos aarch64, 12 CPU`
- OS: `macOS 26.3 (25D125)`

## Dataset Policy

- Benchmark datasets are fetched externally and cached under the active MAG root.
- Default cache root is `~/.mag/benchmarks/`.
- If `~/.mag/` is absent but `~/.romega-memory/` exists, MAG uses the legacy root.
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
| Cached path | `$HOME/.romega-memory/benchmarks/longmemeval/longmemeval_s_cleaned.json` |
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

## LoCoMo10 Retrieval Slice

Command:

```bash
cargo run --release --bin locomo_bench -- --json
```

Dataset source used by this run:

- `https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json`
- Cached at `$HOME/.romega-memory/benchmarks/locomo/locomo10.json` because the active root resolved to the legacy MAG-compatible location

Result:

| Metric | Value |
| --- | --- |
| Samples evaluated | `10` |
| Questions evaluated | `1986` |
| Memories ingested | `5882` |
| Correct | `476` |
| Overall | `23.97%` |
| Total duration | `224.75 s` |
| Average query time | `20.22 ms` |
| Peak RSS | `281184 KB` |

| Category | Score |
| --- | --- |
| category_1 | `20 / 282` |
| category_2 | `16 / 321` |
| category_3 | `6 / 96` |
| category_4 | `291 / 841` |
| category_5 | `143 / 446` |

This is a retrieval-oriented LoCoMo slice, not a full generative benchmark. The README describes it that way intentionally.

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
