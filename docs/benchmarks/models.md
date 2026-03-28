# Embedding Model Comparison
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

LoCoMo word-overlap scoring, 2 samples. Benchmarked 2026-03-19 on macOS aarch64.

ONNX models use int8 quantization unless marked ¹ (no pre-built int8 available). API models are unquantized. Temporal Reasoning is 91.5% for every model and excluded. Scores across models within ~1 pp are within benchmark variance (304 questions, ~1.5% SE).

| Model | Params | Dim | WO% | EvRec% | 1-Hop | Multi-Hop | Open | Adv | AvgEmb | File | RAM |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| granite-embedding-30m-english ¹ | 30M | 384 | 90.5% | 87.5% | 88.9% | 76.9% | 91.7% | 91.1% | 3.8 ms | 116 MB | 350 MB |
| snowflake-arctic-embed-xs int8 | 22M | 384 | 90.2% | 88.7% | 87.0% | 76.9% | 92.7% | 89.5% | 3.9 ms | 22 MB | 137 MB |
| e5-small-v2 int8 | 33M | 384 | 90.8% | 88.6% | 88.4% | 73.1% | 93.0% | 91.1% | 4.8 ms | 32 MB | 152 MB |
| all-MiniLM-L6-v2 int8 | 22M | 384 | 91.3% | 89.2% | 88.5% | 76.9% | 93.1% | 92.3% | 7.4 ms | 22 MB | 95 MB |
| **bge-small-en-v1.5 int8** *(default)* | 33M | 384 | 91.1% | 90.2% | 87.6% | 75.6% | 94.0% | 90.9% | 7.0 ms | 32 MB | 180 MB |
| snowflake-arctic-embed-s int8 | 33M | 384 | 90.8% | 87.8% | 89.5% | 73.1% | 93.0% | 90.8% | 7.8 ms | 32 MB | 178 MB |
| bge-base-en-v1.5 int8 | 109M | 768 | 91.8% | 90.4% | 87.1% | 76.9% | 94.9% | 92.7% | 10.5 ms | 105 MB | 265 MB |
| gte-small int8 | 33M | 384 | 90.9% | 89.5% | 86.2% | 73.1% | 94.0% | 91.7% | 11.7 ms | 32 MB | 162 MB |
| all-MiniLM-L12-v2 int8 | 33M | 384 | 91.1% | 90.4% | 86.8% | 75.6% | 94.3% | 90.9% | 12.3 ms | 32 MB | 158 MB |
| nomic-embed-text-v1.5 int8 | 137M | 768 | 90.0% | 86.6% | 88.4% | 74.4% | 90.8% | 91.0% | 42 ms | 131 MB | 351 MB |
| voyage-4-nano int8 512-dim | — | 512 | 91.3% | 91.6% | 88.8% | 75.6% | 93.7% | 91.6% | 58 ms | — | — |
| voyage-4-nano int8/fp32 1024-dim | — | 1024 | 91.8% | 91.3% | 93.5% | 75.6% | 93.3% | 91.6% | 82–172 ms | — | — |
| voyage-4-lite (API) | — | 1024 | 91.1% | 91.0% | 91.1% | 73.1% | 93.4% | 90.2% | 304 ms | — | — |
| voyage-4 (API) | — | 1024 | 92.0% | 92.7% | 92.3% | 75.6% | 94.8% | 90.6% | 297 ms | — | — |
| text-embedding-3-large (API) | — | 3072 | 93.0% | 93.4% | 94.6% | 74.4% | 95.3% | 93.1% | 444 ms | — | — |

¹ FP32 (no pre-built int8 ONNX available).

## Key Takeaways

- **bge-small-en-v1.5** is the default (Apache 2.0, Xenova int8). 32 MB on disk, 180 MB peak RSS — a 35% reduction vs the previous FP32 default (277 MB) with identical quality.
- **all-MiniLM-L6-v2 int8** is the lightest option at 22 MB / 95 MB RSS with equivalent quality.
- **bge-base-en-v1.5 int8** is the best local ONNX model at +0.7 pp for only 1.4× the latency.
- Switching embedding models requires re-embedding stored data — see [issue #89](https://github.com/George-RD/mag/issues/89).
- Multi-hop is stuck at 73–77% across all models — architectural issue tracked in [issue #84](https://github.com/George-RD/mag/issues/84).
