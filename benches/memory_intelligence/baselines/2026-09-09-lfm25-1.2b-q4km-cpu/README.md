# LFM2.5 1.2B Q4_K_M CPU baseline — 9 September 2026

**Result: 3/14 exact task successes (21.4%), all negative controls. No positive
item matched. This configuration does not pass the production-ingestion quality
gate.** This is evidence for debugging the selected runtime/model combination,
not a ranking of the model family or a held-out quality claim.

## Evidence and reproduction

`evidence.zip` is the original 7,638-byte Actions artifact, imported without
transcription. It contains `run.json`, `scorecard.json`, `pin.json`, `cpu.json`,
and `toolchain.txt`. Its SHA-256 is
`01ed0eecc77d96b29efd5fcc596d3f23134d0806d6500c5a3148d3099f17d7dd`.
No executable, model weights, credentials, or private memory are included.

- Producing commit: `6a3c3d46b83c73a0a2ce12ba136c41b4b0811824`.
- Actions run: https://github.com/George-RD/mag/actions/runs/34308189840
  (job `102329169240`, artifact `10087628388`).
- Dataset digest: `7a001a0aed48bc0e457c155b6ba0780343bb528c661b7473c79dd653a76ce26e`.
- Model: LFM2.5-1.2B-Instruct Q4_K_M, 730,895,168 bytes; exact revision,
  checksum and licence are in the archived pin and run profile.
- Runtime: llama.cpp `304665fe7ac957df95e3ff8c8c4ffdf92dd6ffa3`;
  observed MAG/server binary hashes are in `run.json`.

From the repository root, use a fresh temporary directory:

```bash
out=$(mktemp -d)
python3 -m zipfile -e \
  benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu/evidence.zip \
  "$out"
python3 benches/memory_intelligence/evaluate.py score \
  benches/memory_intelligence/dataset.v1.json "$out/run.json" \
  --output "$out/recomputed.json"
```

The scorer output was independently recomputed locally and exactly matches the
archived scorecard as JSON. A hermetic regression repeats that comparison and
checks archive integrity. Passing that test proves artifact/scorer consistency,
not that the model passed its tasks. Future experiments belong in new dated
baseline directories; do not replace this weak result with a tuned rerun.

## Observed outcomes

| Observation | Result |
| --- | --- |
| Attempts | 14, one per case, no retry or output repair |
| Schema-valid outputs | 11/14 (78.6%) |
| Exact task successes | 3/14 (21.4%); the three negative controls |
| Positive item matches | 0 of 11 expected items |
| Content / grounded recall and F1 | 0 |
| Per-attempt wall latency p50 / p95 | 2,976.193 / 4,464.258 ms; 14 samples |
| Server readiness | 873.671 ms |
| Observed server peak RAM | 1,372,626,944 bytes (about 1.278 GiB); 925 samples |
| Tokens and isolated model load time | Unmeasured; null |

Eight positive cases returned empty items. The grouping response repeated the
value `Orbit` with individual citations, violating the unique-value contract.
The same-time contradiction and Arabic-to-English cases failed JSON capture.
The capture error does not distinguish JSON syntax from UTF-8 decoding; it must
not be interpreted as proof of a Unicode bug. Every failure remains recorded.

The runner exposed four logical CPUs on an AMD EPYC 7763 host; the server used
two threads and no GPU. This is one hosted Linux run, not a hardware survey.
Readiness includes initialization, default warmup and probes, excludes staging,
and is not cold-cache startup. RAM is Linux server-process VmHWM, not total
application/host/GPU use. Per-case wall time includes MAG process/HTTP overhead
against a warm server. See [the runner methodology](../../LOCAL_BASELINE.md).

## Decision

P0 now has an end-to-end local runtime scorecard with verified model-file bytes.
P1 remains a separate quality gate: do not enable production ingestion on this
result. First inspect the rendered request/chat-template/output contract and
empty-item behavior; model limitations are another explanation, not yet isolated.
Use the existing `todo.wire-lfm25-production-ingestion`, preserve this baseline,
and qualify improvements on additional held-out cases before model promotion.
